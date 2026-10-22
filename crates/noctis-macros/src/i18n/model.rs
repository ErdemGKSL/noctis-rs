//! The pure half of `i18n!`: YAML parsing, every file and key check, and
//! row resolution, from `(file name, contents)` pairs.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use noctis_rules::{FRAMEWORK_KEYS, Locale, RuleId, placeholder_names};
use yaml_rust2::Yaml;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// The root maps reserved for command and framework localization.
const RESERVED: [&str; 4] = ["cmd", "cmdword", "ctxcmd", "noctis"];

/// Segments that cannot be accessor or parameter names.
const FORBIDDEN: [&str; 3] = ["self", "super", "crate"];

/// The most missing keys one LOC-04 warning lists.
const LISTED_MISSING: usize = 10;

/// The parameters of a content leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Params {
    /// Named placeholders, in order of first appearance (empty: none).
    Named(Vec<String>),
    /// `{0}`..`{n-1}`: `n` positional parameters `arg0`..`argN`.
    Positional(usize),
}

impl Params {
    /// The placeholder names, in parameter order (`"0"`, `"1"`, … for
    /// positional ones).
    pub(crate) fn names(&self) -> Vec<String> {
        match self {
            Self::Named(names) => names.clone(),
            Self::Positional(n) => (0..*n).map(|i| i.to_string()).collect(),
        }
    }
}

/// A content key of the default locale.
#[derive(Debug, Clone)]
pub(crate) struct Leaf {
    /// The dotted key.
    pub(crate) key: String,
    /// The default-locale text.
    pub(crate) value: String,
    /// Its parameters.
    pub(crate) params: Params,
    /// The `keys::` const name.
    pub(crate) const_name: String,
}

/// A map of the default locale's content tree.
#[derive(Debug, Default)]
pub(crate) struct Node {
    /// Child maps, by segment.
    pub(crate) children: BTreeMap<String, Node>,
    /// Leaves, by segment: the index into [`Model::content`].
    pub(crate) leaves: BTreeMap<String, usize>,
}

/// One locale file.
#[derive(Debug)]
pub(crate) struct File {
    /// Its locale.
    pub(crate) locale: Locale,
    /// Every key written in it, reserved families included.
    pub(crate) entries: BTreeMap<String, String>,
}

/// The checked locale files of one invocation.
#[derive(Debug)]
pub(crate) struct Model {
    /// The default locale.
    pub(crate) default: Locale,
    /// The files, in [`Locale::ALL`] order.
    pub(crate) files: Vec<File>,
    /// The content keys of the default file, sorted by key.
    pub(crate) content: Vec<Leaf>,
    /// The content tree of the default file.
    pub(crate) tree: Node,
    /// One row per file (in `files` order): the resolved text of each
    /// content key.
    pub(crate) rows: Vec<Vec<String>>,
    /// The row index of each locale, in [`Locale::ALL`] order.
    pub(crate) row_of: Vec<usize>,
    /// The placeholder names of every content and `noctis.*` key, sorted.
    pub(crate) placeholders: Vec<(String, Vec<String>)>,
    /// The LOC-04 messages, one per partial file.
    pub(crate) warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

/// The kind of a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scalar {
    Str,
    Int,
    Real,
    Bool,
    Null,
}

/// A parsed YAML node.
#[derive(Debug, Clone)]
enum Y {
    Scalar(Scalar, String),
    Seq,
    Map(Vec<(Y, Y)>),
    Alias,
}

/// One open collection.
enum Frame {
    Seq,
    Map {
        entries: Vec<(Y, Y)>,
        key: Option<Y>,
        seen: HashSet<(Scalar, String)>,
    },
}

/// Builds [`Y`] trees from parser events, keeping aliases and rejecting
/// duplicate keys.
#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    docs: Vec<Y>,
    error: Option<String>,
}

fn scalar_kind(value: &str, style: TScalarStyle, tag: Option<&Tag>) -> Scalar {
    if style != TScalarStyle::Plain {
        return Scalar::Str;
    }
    if let Some(tag) = tag {
        if tag.handle != "tag:yaml.org,2002:" {
            return Scalar::Str;
        }
        return match tag.suffix.as_str() {
            "bool" => Scalar::Bool,
            "int" => Scalar::Int,
            "float" => Scalar::Real,
            "null" => Scalar::Null,
            _ => Scalar::Str,
        };
    }
    match Yaml::from_str(value) {
        Yaml::Integer(_) => Scalar::Int,
        Yaml::Real(_) => Scalar::Real,
        Yaml::Boolean(_) => Scalar::Bool,
        Yaml::Null => Scalar::Null,
        _ => Scalar::Str,
    }
}

impl Builder {
    fn push(&mut self, node: Y, mark: Marker) {
        let Some(top) = self.stack.last_mut() else {
            self.docs.push(node);
            return;
        };
        match top {
            Frame::Seq => {}
            Frame::Map { entries, key, seen } => match key.take() {
                None => *key = Some(node),
                Some(k) => {
                    if let Y::Scalar(kind, text) = &k
                        && !seen.insert((*kind, text.clone()))
                        && self.error.is_none()
                    {
                        self.error = Some(format!(
                            "`{text}`: duplicated key in mapping at line {} column {}",
                            mark.line(),
                            mark.col() + 1
                        ));
                    }
                    entries.push((k, node));
                }
            },
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        match ev {
            Event::Scalar(value, style, _, tag) => {
                let kind = scalar_kind(&value, style, tag.as_ref());
                self.push(Y::Scalar(kind, value), mark);
            }
            Event::Alias(_) => self.push(Y::Alias, mark),
            Event::SequenceStart(..) => self.stack.push(Frame::Seq),
            Event::MappingStart(..) => self.stack.push(Frame::Map {
                entries: Vec::new(),
                key: None,
                seen: HashSet::new(),
            }),
            Event::SequenceEnd | Event::MappingEnd => {
                let node = match self.stack.pop() {
                    Some(Frame::Map { entries, .. }) => Y::Map(entries),
                    _ => Y::Seq,
                };
                self.push(node, mark);
            }
            _ => {}
        }
    }
}

/// Parses one file into its root map entries.
fn parse_yaml(text: &str) -> Result<Vec<(Y, Y)>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut builder = Builder::default();
    let mut parser = Parser::new(text.chars());
    if let Err(e) = parser.load(&mut builder, true) {
        let m = e.marker();
        return Err(format!(
            "{} at line {} column {}",
            e.info(),
            m.line(),
            m.col() + 1
        ));
    }
    if let Some(e) = builder.error {
        return Err(e);
    }
    let mut docs = builder.docs.into_iter();
    let root = docs.next();
    if docs.next().is_some() {
        return Err("a locale file holds one YAML document".to_owned());
    }
    match root {
        None | Some(Y::Scalar(Scalar::Null, _)) => Ok(Vec::new()),
        Some(Y::Map(entries)) => Ok(entries),
        Some(_) => Err("the root must be a map of keys".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Key tree
// ---------------------------------------------------------------------------

fn is_key_segment(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `[a-z][a-z0-9_]*`.
fn is_snake(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn join(path: &str, segment: &str) -> String {
    if path.is_empty() {
        segment.to_owned()
    } else {
        format!("{path}.{segment}")
    }
}

/// Walks one file's tree, collecting its entries and problems.
struct Walk {
    entries: BTreeMap<String, String>,
    errors: Vec<String>,
}

impl Walk {
    /// Walks a map at `path` (`""` for the root); `content` is false inside
    /// a reserved family.
    fn map(&mut self, path: &str, entries: Vec<(Y, Y)>, content: bool) {
        for (key, value) in entries {
            let segment = match key {
                Y::Scalar(Scalar::Str, s) => s,
                Y::Scalar(_, text) => {
                    self.error(path, format!("keys must be strings; quote `{text}`"));
                    continue;
                }
                _ => {
                    self.error(path, "keys must be strings".to_owned());
                    continue;
                }
            };
            let full = join(path, &segment);
            if !is_key_segment(&segment) {
                self.errors.push(format!(
                    "`{full}`: a key segment is ASCII letters, digits, `_` or `-`; nest maps instead of dots"
                ));
                continue;
            }
            let reserved = path.is_empty() && RESERVED.contains(&segment.as_str());
            if reserved {
                match value {
                    Y::Map(children) if !children.is_empty() => {
                        self.map(&full, children, false);
                    }
                    Y::Map(_) => self
                        .errors
                        .push(format!("`{full}` is an empty map; remove it or add keys")),
                    _ => self.errors.push(reserved_not_map(&segment)),
                }
                continue;
            }
            if content {
                if !is_snake(&segment) {
                    self.errors.push(format!(
                        "`{full}`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)"
                    ));
                    continue;
                }
                if FORBIDDEN.contains(&segment.as_str()) {
                    self.errors
                        .push(format!("`{full}`: `{segment}` cannot be an accessor name"));
                    continue;
                }
            }
            self.value(&full, value, content && path.is_empty() && segment == "of");
        }
    }

    fn value(&mut self, full: &str, value: Y, root_of: bool) {
        match value {
            Y::Scalar(Scalar::Str, text) => {
                if root_of {
                    self.errors.push(format!(
                        "`{full}`: a root-level key cannot be named `of` (it is `Strings::of`)"
                    ));
                } else {
                    self.entries.insert(full.to_owned(), text);
                }
            }
            Y::Scalar(Scalar::Null, _) => self.errors.push(format!("`{full}` has no value")),
            Y::Scalar(kind, _) => {
                let found = match kind {
                    Scalar::Int => "an integer",
                    Scalar::Real => "a real number",
                    _ => "a boolean",
                };
                self.errors.push(format!(
                    "`{full}` must be a string, found {found}; quote it"
                ));
            }
            Y::Seq => self
                .errors
                .push(format!("`{full}` must be a string, found a sequence")),
            Y::Alias => self
                .errors
                .push(format!("`{full}`: YAML aliases are not supported")),
            Y::Map(children) if children.is_empty() => self
                .errors
                .push(format!("`{full}` is an empty map; remove it or add keys")),
            Y::Map(children) => self.map(full, children, true),
        }
    }

    fn error(&mut self, path: &str, message: String) {
        if path.is_empty() {
            self.errors.push(message);
        } else {
            self.errors.push(format!("`{path}`: {message}"));
        }
    }
}

fn reserved_not_map(family: &str) -> String {
    let what = if family == "noctis" {
        "framework strings"
    } else {
        "command localization"
    };
    format!("`{family}` is reserved for {what} and must be a map")
}

/// Whether `key` is in a reserved family.
fn is_reserved(key: &str) -> bool {
    let first = key.split('.').next().unwrap_or_default();
    RESERVED.contains(&first)
}

// ---------------------------------------------------------------------------
// Placeholders
// ---------------------------------------------------------------------------

/// The placeholder names of `value`, each once, in order of first
/// appearance.
fn unique_names(value: &str) -> Vec<&str> {
    let mut seen = Vec::new();
    for name in placeholder_names(value) {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// `` `{a}`, `{b}` ``, or `(none)`.
fn braces<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let list: Vec<String> = names.into_iter().map(|n| format!("`{{{n}}}`")).collect();
    if list.is_empty() {
        "(none)".to_owned()
    } else {
        list.join(", ")
    }
}

/// The parameters of a content leaf, or the problem.
fn params(key: &str, value: &str) -> Result<Params, String> {
    let names = unique_names(value);
    let mut snake = Vec::new();
    let mut positional = Vec::new();
    for name in &names {
        if name.bytes().all(|b| b.is_ascii_digit()) {
            positional.push(*name);
        } else if FORBIDDEN.contains(name) {
            return Err(format!("`{key}`: `{{{name}}}` cannot be a parameter"));
        } else if is_snake(name) {
            snake.push((*name).to_owned());
        } else {
            return Err(format!(
                "`{key}`: placeholder `{{{name}}}` must be lowercase snake_case or an index"
            ));
        }
    }
    if !snake.is_empty() && !positional.is_empty() {
        return Err(format!("`{key}` mixes positional and named placeholders"));
    }
    if positional.is_empty() {
        return Ok(Params::Named(snake));
    }
    let mut indexes = BTreeSet::new();
    for name in positional {
        if name.len() > 1 && name.starts_with('0') {
            return Err(format!("`{key}`: `{{{name}}}` is not a canonical index"));
        }
        indexes.insert(name.parse::<usize>().unwrap_or(usize::MAX));
    }
    let n = indexes.len();
    if let Some(missing) = (0..n).find(|i| !indexes.contains(i)) {
        return Err(format!(
            "`{key}`: positional placeholders must be `{{0}}`..`{{n}}` without gaps; missing `{{{missing}}}`"
        ));
    }
    Ok(Params::Positional(n))
}

/// The `keys::` const name of `key`.
fn const_name(key: &str) -> String {
    key.replace('.', "_").to_ascii_uppercase()
}

// ---------------------------------------------------------------------------
// Compile
// ---------------------------------------------------------------------------

/// The LOC-01 help for a stem that is not a code.
fn stem_help(suggestions: &[Locale]) -> String {
    let names: Vec<String> = suggestions.iter().map(|l| format!("`{l}.yaml`")).collect();
    match names.split_last() {
        None => "use one of Discord's 32 codes, such as `en-US.yaml`, `tr.yaml` or `pt-BR.yaml`"
            .to_owned(),
        Some((last, [])) => format!("name it {last}"),
        Some((last, rest)) => format!("name it {} or {last}", rest.join(", ")),
    }
}

/// A file's locale, its shown path and its flattened entries (`None` when
/// it does not parse).
type Parsed = (Locale, String, Option<BTreeMap<String, String>>);

/// Builds the model of `files` (`(file name, contents)` pairs whose names
/// end in `.yaml`), or every problem found.
pub(crate) fn compile(
    files: &[(String, String)],
    default: Locale,
    dir: &str,
) -> Result<Model, Vec<String>> {
    let mut errors = Vec::new();
    let mut parsed: Vec<Parsed> = Vec::new();
    for (name, text) in files {
        let stem = name.strip_suffix(".yaml").unwrap_or(name);
        let locale = match stem.parse::<Locale>() {
            Ok(locale) => locale,
            Err(e) => {
                errors.push(format!(
                    "[{}] locale file `{dir}/{name}` is not named by a Discord locale code; help: {}",
                    RuleId::LOC_01,
                    stem_help(e.suggestions())
                ));
                continue;
            }
        };
        let shown = format!("{dir}/{name}");
        match parse_yaml(text) {
            Err(e) => {
                errors.push(format!("i18n!: `{shown}`: {e}"));
                parsed.push((locale, shown, None));
            }
            Ok(root) => {
                let mut walk = Walk {
                    entries: BTreeMap::new(),
                    errors: Vec::new(),
                };
                walk.map("", root, true);
                framework_checks(&walk.entries, &mut walk.errors);
                let ok = walk.errors.is_empty();
                errors.extend(
                    walk.errors
                        .into_iter()
                        .map(|e| format!("i18n!: `{shown}`: {e}")),
                );
                parsed.push((locale, shown, ok.then_some(walk.entries)));
            }
        }
    }
    parsed.sort_by_key(|(locale, ..)| Locale::ALL.iter().position(|l| l == locale));
    let Some(default_index) = parsed.iter().position(|(l, ..)| *l == default) else {
        errors.push(format!(
            "i18n!: the default locale `{default}` has no file `{dir}/{default}.yaml`"
        ));
        return Err(errors);
    };
    let Some(default_entries) = parsed[default_index].2.clone() else {
        return Err(errors);
    };
    let content = match content_leaves(&default_entries, &parsed[default_index].1) {
        Ok(content) => content,
        Err(mut e) => {
            errors.append(&mut e);
            return Err(errors);
        }
    };
    let content_keys: BTreeSet<&str> = content.iter().map(|l| l.key.as_str()).collect();
    let mut warnings = Vec::new();
    for (i, (_, shown, entries)) in parsed.iter().enumerate() {
        let Some(entries) = entries else { continue };
        if i == default_index {
            continue;
        }
        translation_checks(
            shown,
            entries,
            &content,
            &content_keys,
            default,
            &mut errors,
        );
        let missing: Vec<&str> = content
            .iter()
            .map(|l| l.key.as_str())
            .filter(|k| !entries.contains_key(*k))
            .collect();
        if !missing.is_empty() {
            warnings.push(loc_04(shown, &missing, default));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let files: Vec<File> = parsed
        .into_iter()
        .map(|(locale, _, entries)| File {
            locale,
            entries: entries.unwrap_or_default(),
        })
        .collect();
    Ok(resolve(default, files, content, warnings))
}

/// `noctis.*` keys must be framework keys with their placeholders.
fn framework_checks(entries: &BTreeMap<String, String>, errors: &mut Vec<String>) {
    for (key, value) in entries.range("noctis.".to_owned()..) {
        if !key.starts_with("noctis.") {
            break;
        }
        let Some((_, listed)) = FRAMEWORK_KEYS.iter().find(|(k, _)| k == key) else {
            errors.push(format!(
                "`{key}` is not a framework key; `noctis.` is reserved: use your own namespace"
            ));
            continue;
        };
        let found = unique_names(value);
        let same = found.len() == listed.len() && found.iter().all(|n| listed.contains(n));
        if !same {
            let found = if found.is_empty() {
                "none".to_owned()
            } else {
                braces(found)
            };
            errors.push(format!(
                "`{key}` must use exactly the placeholders {}; found {found}",
                braces(listed.iter().copied())
            ));
        }
    }
}

/// The content leaves of the default file, with their parameters and
/// const names.
fn content_leaves(
    entries: &BTreeMap<String, String>,
    shown: &str,
) -> Result<Vec<Leaf>, Vec<String>> {
    let mut errors = Vec::new();
    let mut leaves = Vec::new();
    let mut consts: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in entries.iter().filter(|(k, _)| !is_reserved(k)) {
        let params = match params(key, value) {
            Ok(params) => params,
            Err(e) => {
                errors.push(format!("i18n!: `{shown}`: {e}"));
                continue;
            }
        };
        let name = const_name(key);
        if let Some(first) = consts.get(&name) {
            errors.push(format!(
                "i18n!: keys `{first}` and `{key}` both map to `keys::{name}`"
            ));
            continue;
        }
        consts.insert(name.clone(), key.clone());
        leaves.push(Leaf {
            key: key.clone(),
            value: value.clone(),
            params,
            const_name: name,
        });
    }
    if errors.is_empty() {
        Ok(leaves)
    } else {
        Err(errors)
    }
}

/// The checks of a non-default file against the default one.
fn translation_checks(
    shown: &str,
    entries: &BTreeMap<String, String>,
    content: &[Leaf],
    content_keys: &BTreeSet<&str>,
    default: Locale,
    errors: &mut Vec<String>,
) {
    for (key, value) in entries.iter().filter(|(k, _)| !is_reserved(k)) {
        if !content_keys.contains(key.as_str()) {
            errors.push(format!(
                "i18n!: `{shown}`: `{key}` is not in the default locale `{default}.yaml`"
            ));
            continue;
        }
        let Some(leaf) = content.iter().find(|l| &l.key == key) else {
            continue;
        };
        let want = unique_names(&leaf.value);
        let got = unique_names(value);
        let missing: Vec<&str> = want.iter().copied().filter(|n| !got.contains(n)).collect();
        let extra: Vec<&str> = got.iter().copied().filter(|n| !want.contains(n)).collect();
        if missing.is_empty() && extra.is_empty() {
            continue;
        }
        let mut message = format!(
            "i18n!: `{shown}`: `{key}` must use the placeholders of the default locale {}",
            braces(want.iter().copied())
        );
        let mut sep = "; ";
        if !missing.is_empty() {
            let _ = write!(message, "{sep}missing {}", braces(missing));
            sep = ", ";
        }
        if !extra.is_empty() {
            let _ = write!(message, "{sep}extra {}", braces(extra));
        }
        errors.push(message);
    }
}

/// The LOC-04 message of a file lacking `missing` (sorted).
fn loc_04(shown: &str, missing: &[&str], default: Locale) -> String {
    let listed: Vec<String> = missing
        .iter()
        .take(LISTED_MISSING)
        .map(|k| format!("`{k}`"))
        .collect();
    let mut list = listed.join(", ");
    if missing.len() > LISTED_MISSING {
        let _ = write!(list, ", and {} more", missing.len() - LISTED_MISSING);
    }
    format!(
        "[{}] `{shown}` lacks {} key(s) of the default locale `{default}.yaml`: {list}; they fall back at runtime",
        RuleId::LOC_04,
        missing.len()
    )
}

/// Resolves the rows, the tree and the placeholder table.
fn resolve(default: Locale, files: Vec<File>, content: Vec<Leaf>, warnings: Vec<String>) -> Model {
    let index_of = |locale: Locale| files.iter().position(|f| f.locale == locale);
    let default_file = index_of(default).unwrap_or(0);
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|file| {
            let sibling = file.locale.sibling().and_then(index_of);
            content
                .iter()
                .map(|leaf| {
                    file.entries
                        .get(&leaf.key)
                        .or_else(|| sibling.and_then(|s| files[s].entries.get(&leaf.key)))
                        .unwrap_or(&leaf.value)
                        .clone()
                })
                .collect()
        })
        .collect();
    let row_of = Locale::ALL
        .iter()
        .map(|locale| {
            index_of(*locale)
                .or_else(|| locale.sibling().and_then(index_of))
                .unwrap_or(default_file)
        })
        .collect();
    let mut tree = Node::default();
    for (i, leaf) in content.iter().enumerate() {
        let mut segments: Vec<&str> = leaf.key.split('.').collect();
        let last = segments.pop().unwrap_or_default();
        let mut node = &mut tree;
        for segment in segments {
            node = node.children.entry(segment.to_owned()).or_default();
        }
        node.leaves.insert(last.to_owned(), i);
    }
    let mut placeholders: BTreeMap<String, Vec<String>> = content
        .iter()
        .map(|l| (l.key.clone(), l.params.names()))
        .collect();
    for file in &files {
        for key in file.entries.keys().filter(|k| k.starts_with("noctis.")) {
            if let Some((_, names)) = FRAMEWORK_KEYS.iter().find(|(k, _)| k == key) {
                placeholders.insert(key.clone(), names.iter().map(|n| (*n).to_owned()).collect());
            }
        }
    }
    Model {
        default,
        files,
        content,
        tree,
        rows,
        row_of,
        placeholders: placeholders.into_iter().collect(),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "locales";

    fn files(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, t)| ((*n).to_owned(), (*t).to_owned()))
            .collect()
    }

    fn ok(list: &[(&str, &str)]) -> Model {
        match compile(&files(list), Locale::EnUs, D) {
            Ok(model) => model,
            Err(e) => panic!("{e:?}"),
        }
    }

    fn errs(list: &[(&str, &str)]) -> Vec<String> {
        compile(&files(list), Locale::EnUs, D)
            .err()
            .unwrap_or_default()
    }

    /// The single error of a default file holding `yaml`.
    fn err(yaml: &str) -> String {
        let e = errs(&[("en-US.yaml", yaml)]);
        assert_eq!(e.len(), 1, "{e:?}");
        e[0].clone()
    }

    fn row(model: &Model, locale: Locale, key: &str) -> String {
        let i = model.content.iter().position(|l| l.key == key).unwrap();
        let r = model.row_of[Locale::ALL.iter().position(|l| *l == locale).unwrap()];
        model.rows[r][i].clone()
    }

    /// Matrix 1.
    #[test]
    fn happy_path() {
        let m = ok(&[
            ("en-US.yaml", "common: {refresh: Refresh}"),
            ("tr.yaml", "common: {refresh: Yenile}"),
        ]);
        assert_eq!(m.content.len(), 1);
        assert_eq!(m.content[0].const_name, "COMMON_REFRESH");
        assert_eq!(row(&m, Locale::Tr, "common.refresh"), "Yenile");
        assert_eq!(row(&m, Locale::De, "common.refresh"), "Refresh");
        assert_eq!(
            m.files.iter().map(|f| f.locale).collect::<Vec<_>>(),
            [Locale::EnUs, Locale::Tr]
        );
        assert!(m.tree.children["common"].leaves.contains_key("refresh"));
        assert!(m.warnings.is_empty());
    }

    /// Matrix 9.
    #[test]
    fn default_file_missing() {
        let want = "i18n!: the default locale `en-US` has no file `locales/en-US.yaml`";
        assert_eq!(errs(&[]), [want]);
        assert_eq!(errs(&[("tr.yaml", "a: b")]), [want]);
    }

    /// Matrix 10-12, 14.
    #[test]
    fn stems_must_be_codes() {
        let e = errs(&[("en-US.yaml", ""), ("en.yaml", "")]);
        assert_eq!(
            e,
            [
                "[LOC-01] locale file `locales/en.yaml` is not named by a Discord locale code; help: name it `en-GB.yaml` or `en-US.yaml`"
            ]
        );
        let e = errs(&[("en-US.yaml", ""), ("EN-us.yaml", "")]);
        assert!(
            e[0].contains("`locales/EN-us.yaml`")
                && e[0].ends_with("name it `en-GB.yaml` or `en-US.yaml`"),
            "{e:?}"
        );
        let help = |stem: &str| {
            let e = errs(&[("en-US.yaml", ""), (&format!("{stem}.yaml"), "")]);
            e[0].split("help: ").nth(1).unwrap_or_default().to_owned()
        };
        assert_eq!(help("pt"), "name it `pt-BR.yaml`");
        assert_eq!(help("zh"), "name it `zh-CN.yaml` or `zh-TW.yaml`");
        assert_eq!(
            help("xx"),
            "use one of Discord's 32 codes, such as `en-US.yaml`, `tr.yaml` or `pt-BR.yaml`"
        );
        assert_eq!(help(".tr"), help("xx"));
        assert_eq!(help("en_US"), "name it `en-GB.yaml` or `en-US.yaml`");
        assert_eq!(
            stem_help(&[Locale::EnGb, Locale::EnUs, Locale::Tr]),
            "name it `en-GB.yaml`, `en-US.yaml` or `tr.yaml`"
        );
    }

    /// Matrix 17-22.
    #[test]
    fn file_shapes() {
        assert_eq!(ok(&[("en-US.yaml", "\u{feff}a: b")]).content.len(), 1);
        let e = err("a: [");
        assert!(
            e.starts_with("i18n!: `locales/en-US.yaml`: ") && e.contains(" at line "),
            "{e}"
        );
        let e = err("a: x\na: y");
        assert!(e.contains("duplicated key in mapping at line 2"), "{e}");
        assert_eq!(
            err("a: x\n---\nb: y"),
            "i18n!: `locales/en-US.yaml`: a locale file holds one YAML document"
        );
        for empty in ["", "# only a comment\n", "~", "null"] {
            let m = ok(&[("en-US.yaml", empty)]);
            assert!(
                m.content.is_empty() && m.tree.children.is_empty(),
                "{empty:?}"
            );
        }
        for root in ["- a", "hello"] {
            assert_eq!(
                err(root),
                "i18n!: `locales/en-US.yaml`: the root must be a map of keys"
            );
        }
    }

    /// Matrix 23-28.
    #[test]
    fn value_and_key_errors() {
        let p = "i18n!: `locales/en-US.yaml`: ";
        let cases = [
            ("n: 3", "`n` must be a string, found an integer; quote it"),
            ("b: true", "`b` must be a string, found a boolean; quote it"),
            (
                "r: 1.5",
                "`r` must be a string, found a real number; quote it",
            ),
            ("a:", "`a` has no value"),
            ("a: [x]", "`a` must be a string, found a sequence"),
            ("x: &v y\na: *v", "`a`: YAML aliases are not supported"),
            ("a: {}", "`a` is an empty map; remove it or add keys"),
            ("1: x", "keys must be strings; quote `1`"),
            ("true: x", "keys must be strings; quote `true`"),
            (
                "\"a.b\": x",
                "`a.b`: a key segment is ASCII letters, digits, `_` or `-`; nest maps instead of dots",
            ),
            (
                "\"a b\": x",
                "`a b`: a key segment is ASCII letters, digits, `_` or `-`; nest maps instead of dots",
            ),
            (
                "Setup: x",
                "`Setup`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)",
            ),
            (
                "a-b: x",
                "`a-b`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)",
            ),
            (
                "1st: x",
                "`1st`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)",
            ),
            (
                "_a: x",
                "`_a`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)",
            ),
        ];
        for (yaml, want) in cases {
            assert_eq!(err(yaml), format!("{p}{want}"), "{yaml}");
        }
        assert_eq!(ok(&[("en-US.yaml", "x: \"3\"")]).content[0].value, "3");
    }

    /// Matrix 29-31.
    #[test]
    fn keywords_forbidden_segments_and_of() {
        let m = ok(&[("en-US.yaml", "type: x\nprofile: {match: y}\na: {of: z}")]);
        let names: Vec<&str> = m.content.iter().map(|l| l.const_name.as_str()).collect();
        assert_eq!(names, ["A_OF", "PROFILE_MATCH", "TYPE"]);
        let p = "i18n!: `locales/en-US.yaml`: ";
        assert_eq!(
            err("self: x"),
            format!("{p}`self`: `self` cannot be an accessor name")
        );
        assert_eq!(
            err("a: {super: y}"),
            format!("{p}`a.super`: `super` cannot be an accessor name")
        );
        assert_eq!(
            err("crate: z"),
            format!("{p}`crate`: `crate` cannot be an accessor name")
        );
        assert_eq!(
            err("of: x"),
            format!("{p}`of`: a root-level key cannot be named `of` (it is `Strings::of`)")
        );
        assert_eq!(ok(&[("en-US.yaml", "of: {a: x}")]).content[0].key, "of.a");
    }

    /// Matrix 32.
    #[test]
    fn const_name_clash() {
        assert_eq!(
            err("a: {b_c: x}\na_b: {c: y}"),
            "i18n!: keys `a.b_c` and `a_b.c` both map to `keys::A_B_C`"
        );
    }

    /// Matrix 33-38.
    #[test]
    fn reserved_families() {
        let p = "i18n!: `locales/en-US.yaml`: ";
        assert_eq!(
            err("cmd: x"),
            format!("{p}`cmd` is reserved for command localization and must be a map")
        );
        assert_eq!(
            err("cmdword: x"),
            format!("{p}`cmdword` is reserved for command localization and must be a map")
        );
        assert_eq!(
            err("ctxcmd: x"),
            format!("{p}`ctxcmd` is reserved for command localization and must be a map")
        );
        assert_eq!(
            err("noctis: x"),
            format!("{p}`noctis` is reserved for framework strings and must be a map")
        );
        let m = ok(&[(
            "en-US.yaml",
            "cmd: {setup: {description: \"Set {x} up\"}, set-up: {description: y}}\ncmdword: {setup: kurulum}\nctxcmd: {show: {name: Show}}",
        )]);
        assert!(m.content.is_empty());
        assert_eq!(m.files[0].entries["cmd.setup.description"], "Set {x} up");
        assert!(m.files[0].entries.contains_key("cmd.set-up.description"));
        assert!(m.placeholders.is_empty());
        let m = ok(&[
            ("en-US.yaml", ""),
            ("tr.yaml", "noctis: {expired: \"Panel expired\"}"),
        ]);
        assert_eq!(m.files[1].entries["noctis.expired"], "Panel expired");
        assert_eq!(m.placeholders, [("noctis.expired".to_owned(), Vec::new())]);
        assert!(m.warnings.is_empty());
        assert_eq!(
            err("noctis: {wizard: {next: x}}"),
            format!(
                "{p}`noctis.wizard.next` is not a framework key; `noctis.` is reserved: use your own namespace"
            )
        );
        assert_eq!(
            err("noctis: {rate_limited: \"wait {secs}\"}"),
            format!(
                "{p}`noctis.rate_limited` must use exactly the placeholders `{{retry}}`; found `{{secs}}`"
            )
        );
        assert_eq!(
            err("noctis: {expired: \"x {a}\"}"),
            format!("{p}`noctis.expired` must use exactly the placeholders (none); found `{{a}}`")
        );
        assert!(
            ok(&[("en-US.yaml", "noctis: {rate_limited: \"{retry} {retry}\"}")])
                .warnings
                .is_empty()
        );
    }

    /// Matrix 39-47.
    #[test]
    fn placeholder_grammar() {
        let params_of = |v: &str| params("k", v);
        assert_eq!(
            params_of("Hi {name}, {name}! You have {n}"),
            Ok(Params::Named(vec!["name".into(), "n".into()]))
        );
        assert_eq!(params_of("{1} of {0}"), Ok(Params::Positional(2)));
        assert_eq!(
            params_of("{0} {name}"),
            Err("`k` mixes positional and named placeholders".into())
        );
        assert_eq!(
            params_of("{0} {2}"),
            Err(
                "`k`: positional placeholders must be `{0}`..`{n}` without gaps; missing `{1}`"
                    .into()
            )
        );
        assert_eq!(
            params_of("{01}"),
            Err("`k`: `{01}` is not a canonical index".into())
        );
        assert_eq!(
            params_of("{99999999999999999999}"),
            Err(
                "`k`: positional placeholders must be `{0}`..`{n}` without gaps; missing `{0}`"
                    .into()
            )
        );
        for bad in ["{Name}", "{_x}", "{1a}"] {
            let name = &bad[1..bad.len() - 1];
            assert_eq!(
                params_of(bad),
                Err(format!(
                    "`k`: placeholder `{{{name}}}` must be lowercase snake_case or an index"
                ))
            );
        }
        assert_eq!(
            params_of("{self}"),
            Err("`k`: `{self}` cannot be a parameter".into())
        );
        assert_eq!(params_of("{} {a b} {x"), Ok(Params::Named(vec![])));
        assert_eq!(params_of("{😀}"), Ok(Params::Named(vec![])));
        assert_eq!(params_of("{{n}}"), Ok(Params::Named(vec!["n".into()])));
        assert_eq!(params_of("{type}"), Ok(Params::Named(vec!["type".into()])));
        let m = ok(&[("en-US.yaml", "u: \"çğ😀 {n}\"\ne: \"\"")]);
        assert_eq!(m.content[0].value, "");
        assert_eq!(m.content[1].value, "çğ😀 {n}");
        assert!(err("m: \"{0} {name}\"").ends_with("`m` mixes positional and named placeholders"));
    }

    /// Matrix 48-52.
    #[test]
    fn translations() {
        let e = errs(&[
            ("en-US.yaml", "k: \"{n} left\""),
            ("tr.yaml", "k: \"{count} kaldı\""),
        ]);
        assert_eq!(
            e,
            [
                "i18n!: `locales/tr.yaml`: `k` must use the placeholders of the default locale `{n}`; missing `{n}`, extra `{count}`"
            ]
        );
        assert!(
            ok(&[
                ("en-US.yaml", "k: \"{a} {b}\""),
                ("tr.yaml", "k: \"{b} {a}\"")
            ])
            .warnings
            .is_empty()
        );
        assert_eq!(
            errs(&[("en-US.yaml", "a: x"), ("tr.yaml", "a: y\nx: {y: z}")]),
            ["i18n!: `locales/tr.yaml`: `x.y` is not in the default locale `en-US.yaml`"]
        );
        let m = ok(&[("en-US.yaml", "a: A\nb: B\nc: C"), ("tr.yaml", "c: Ç")]);
        assert_eq!(
            m.warnings,
            [
                "[LOC-04] `locales/tr.yaml` lacks 2 key(s) of the default locale `en-US.yaml`: `a`, `b`; they fall back at runtime"
            ]
        );
        assert_eq!(row(&m, Locale::Tr, "a"), "A");
        assert_eq!(row(&m, Locale::Tr, "c"), "Ç");
        let many: String = (0..13)
            .flat_map(|i| [format!("k{i:02}"), ": x\n".to_owned()])
            .collect();
        let m = ok(&[("en-US.yaml", &many), ("tr.yaml", "")]);
        assert!(
            m.warnings[0].ends_with("`k00`, `k01`, `k02`, `k03`, `k04`, `k05`, `k06`, `k07`, `k08`, `k09`, and 3 more; they fall back at runtime"),
            "{}",
            m.warnings[0]
        );
        assert!(m.warnings[0].contains("lacks 13 key(s)"));
    }

    /// Matrix 53-55.
    #[test]
    fn rows_resolve_through_siblings() {
        let m = ok(&[
            ("en-US.yaml", "a: us\nb: us-b"),
            ("es-ES.yaml", "b: es-b"),
            ("es-419.yaml", "a: latam\nb: latam-b"),
        ]);
        assert_eq!(row(&m, Locale::EnGb, "a"), "us");
        assert_eq!(row(&m, Locale::Es419, "a"), "latam");
        assert_eq!(row(&m, Locale::EsEs, "a"), "latam");
        assert_eq!(row(&m, Locale::EsEs, "b"), "es-b");
        assert_eq!(row(&m, Locale::PtBr, "a"), "us");
        assert_eq!(m.row_of.len(), 32);
        assert!(m.row_of.iter().all(|r| *r < m.rows.len()));
    }

    #[test]
    fn placeholder_table_lists_content_keys() {
        let m = ok(&[("en-US.yaml", "p: \"{1} of {0}\"\ng: \"{name}\"\nplain: x")]);
        assert_eq!(
            m.placeholders,
            [
                ("g".to_owned(), vec!["name".to_owned()]),
                ("p".to_owned(), vec!["0".to_owned(), "1".to_owned()]),
                ("plain".to_owned(), vec![]),
            ]
        );
    }

    #[test]
    fn other_default_locale() {
        let m = compile(
            &files(&[("tr.yaml", "a: b"), ("en-US.yaml", "")]),
            Locale::Tr,
            "l",
        )
        .ok()
        .unwrap();
        assert_eq!(m.default, Locale::Tr);
        assert_eq!(
            m.warnings,
            [
                "[LOC-04] `l/en-US.yaml` lacks 1 key(s) of the default locale `tr.yaml`: `a`; they fall back at runtime"
            ]
        );
    }
}
