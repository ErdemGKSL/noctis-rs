//! One chat-input command tree set: [`validate_tree`] (CMD-01..05, CMD-10..14,
//! REG-04).

use std::collections::HashMap;

use super::{CommandDecl, DeclKind, LocalizedText, from_suffix, source_name, validate_name};
use crate::len::discord_len;
use crate::{Actual, Bound, Limits, RuleId, Violation};

/// A word prefix of at least one handler path.
struct Node<'a> {
    /// The path up to and including this node's word.
    path: &'a str,
    word: &'a str,
    /// 1 for a base command.
    depth: usize,
    children: Vec<usize>,
    /// Indices of the declarations whose path is exactly `path`, in input order.
    decls: Vec<usize>,
    /// The first handler declaration whose path extends below this node.
    first_child_decl: Option<usize>,
}

struct Tree<'d, 'a> {
    decls: &'d [CommandDecl<'a>],
    nodes: Vec<Node<'a>>,
    roots: Vec<usize>,
    limits: &'d Limits,
}

impl<'a> Tree<'_, 'a> {
    fn decl(&self, index: usize) -> &CommandDecl<'a> {
        &self.decls[index]
    }

    fn handlers<'s>(&'s self, node: &'s Node<'a>) -> impl Iterator<Item = usize> + 's {
        node.decls
            .iter()
            .copied()
            .filter(|&i| matches!(self.decl(i).kind, DeclKind::Handler { .. }))
    }

    fn description(&self, node: &Node<'a>) -> Option<LocalizedText<'a>> {
        node.decls.iter().find_map(|&i| self.decl(i).description)
    }

    /// Name, CMD-04, CMD-11/12, CMD-13 for `index` and its subtree, then CMD-05
    /// for a base.
    fn check_node(&self, index: usize, out: &mut Vec<Violation>) {
        let node = &self.nodes[index];
        let limits = self.limits;
        for mut v in validate_name(node.word, limits) {
            node.path.clone_into(&mut v.path);
            out.push(v);
        }

        let first_handler = self.handlers(node).next();
        self.check_description_len(node, first_handler.is_some(), out);
        if let (Some(leaf), Some(child)) = (first_handler, node.first_child_decl) {
            out.push(self.leaf_with_children(node, leaf, child));
        }

        if node.children.len() > limits.command_options {
            out.push(Violation::new(
                RuleId::CMD_13,
                node.path,
                Bound::AtMost(limits.command_options),
                Actual::Count(node.children.len()),
            ));
        }
        if let Some(DeclKind::Handler { options, .. }) = first_handler.map(|i| self.decl(i).kind)
            && options > limits.command_options
        {
            out.push(Violation::new(
                RuleId::CMD_13,
                node.path,
                Bound::AtMost(limits.command_options),
                Actual::Count(options),
            ));
        }

        for &child in &node.children {
            self.check_node(child, out);
        }

        if node.depth == 1 {
            let size = self.size(index);
            if size > limits.command_size {
                out.push(
                    Violation::new(
                        RuleId::CMD_05,
                        node.path,
                        Bound::AtMost(limits.command_size),
                        Actual::Count(size),
                    )
                    .with_note("longest variant per field"),
                );
            }
        }
    }

    /// CMD-04 for the node's description.
    fn check_description_len(&self, node: &Node<'a>, is_handler: bool, out: &mut Vec<Violation>) {
        let limits = self.limits;
        let description_bound = Bound::Between {
            min: 1,
            max: limits.command_description_len,
        };
        let Some(text) = self.description(node) else {
            if is_handler {
                out.push(
                    Violation::new(
                        RuleId::CMD_04,
                        node.path,
                        description_bound,
                        Actual::Count(0),
                    )
                    .with_note("missing description"),
                );
            }
            return;
        };
        let variants = std::iter::once((text.default, "in the default description".to_owned()))
            .chain(
                text.localized
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (*t, format!("in localized description #{i}"))),
            );
        for (variant, note) in variants {
            let len = discord_len(variant);
            if len == 0 || len > limits.command_description_len {
                out.push(
                    Violation::new(
                        RuleId::CMD_04,
                        node.path,
                        description_bound.clone(),
                        Actual::Count(len),
                    )
                    .with_note(note),
                );
            }
        }
    }

    /// CMD-12 (base) or CMD-11 (subcommand) for a handler node with children.
    fn leaf_with_children(&self, node: &Node<'a>, leaf: usize, child: usize) -> Violation {
        let (leaf, child) = (self.decl(leaf), self.decl(child));
        let word = node.word;
        let (rule, describe, leaf_note, child_note) = if node.depth == 1 {
            (
                RuleId::CMD_12,
                "a base command with subcommands is not invokable",
                format!("`{}` makes `{word}` invokable", leaf.path),
                format!("`{}` gives `{word}` subcommands", child.path),
            )
        } else {
            (
                RuleId::CMD_11,
                "a subcommand or a subcommand group, not both",
                format!("`{}` makes `{word}` a subcommand", leaf.path),
                format!("`{}` makes `{word}` a subcommand group", child.path),
            )
        };
        Violation::new(
            rule,
            node.path,
            Bound::Describe(describe),
            Actual::Text(word.to_owned()),
        )
        .with_note(leaf_note + &from_suffix(leaf.source))
        .with_note(child_note + &from_suffix(child.source))
    }

    /// The CMD-05 size of the subtree at `index`.
    fn size(&self, index: usize) -> usize {
        let node = &self.nodes[index];
        let name = node
            .decls
            .iter()
            .flat_map(|&i| self.decl(i).name_localized)
            .map(|name| discord_len(name))
            .fold(discord_len(node.word), usize::max);
        let description = self.description(node).map_or(0, |d| d.longest());
        let options = match self.handlers(node).next().map(|i| self.decl(i).kind) {
            Some(DeclKind::Handler { options_size, .. }) => options_size,
            _ => 0,
        };
        node.children.iter().fold(
            name.saturating_add(description).saturating_add(options),
            |acc, &child| acc.saturating_add(self.size(child)),
        )
    }

    /// REG-04 description conflicts, in pre-order.
    fn check_descriptions(&self, index: usize, out: &mut Vec<Violation>) {
        let node = &self.nodes[index];
        let described: Vec<_> = node
            .decls
            .iter()
            .filter_map(|&i| self.decl(i).description.map(|d| (self.decl(i).source, d)))
            .collect();
        if described.iter().any(|(_, d)| *d != described[0].1) {
            let mut v = Violation::new(
                RuleId::REG_04,
                node.path,
                Bound::Describe("one description per command or group"),
                Actual::Text(node.path.to_owned()),
            );
            for (source, d) in &described {
                v = v.with_note(format!("{} declares `{}`", source_name(source), d.default));
            }
            out.push(v);
        }
        for &child in &node.children {
            self.check_descriptions(child, out);
        }
    }
}

/// Checks one set of chat-input command declarations as a single tree set.
///
/// `scope` is ignored (see [`validate_merge`](super::validate_merge) for
/// per-scope checks). Handler paths build the nodes; meta declarations attach
/// to the node with their exact path. A node's description is the first `Some`
/// in input order; a handler node needs one (CMD-04), a group or base without a
/// handler does not. A node's CMD-05 name length is the longest of its word and
/// every `name_localized` declared for it.
///
/// Checks: CMD-10 per declaration (such a declaration is left out of the
/// tree), REG-04 duplicate handler paths (the first one counts for the budget)
/// and metadata for unregistered paths, the name rules for every node word,
/// CMD-04, CMD-11 (a subcommand that is also a group), CMD-12 (an invokable
/// base with subcommands), CMD-13 (children per node, options per handler),
/// CMD-05 per base, and REG-04 conflicting descriptions. The order is in the
/// [module docs](super).
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{CommandDecl, LocalizedText, validate_tree};
///
/// let d = LocalizedText::new("d");
/// let decls = [
///     CommandDecl::handler("a", "config").description(d),
///     CommandDecl::handler("b", "config show").description(d),
/// ];
/// let errs = validate_tree(&decls, &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::CMD_12);
/// ```
pub fn validate_tree(decls: &[CommandDecl<'_>], limits: &Limits) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    let valid = check_depth(decls, limits, &mut out);
    let tree = Tree::build(decls, &valid, limits, &mut out);
    for &root in &tree.roots {
        tree.check_node(root, &mut out);
    }
    for &root in &tree.roots {
        tree.check_descriptions(root, &mut out);
    }
    if out.is_empty() { Ok(()) } else { Err(out) }
}

/// CMD-10 per declaration; returns the indices of the declarations that pass.
fn check_depth(decls: &[CommandDecl<'_>], limits: &Limits, out: &mut Vec<Violation>) -> Vec<usize> {
    let mut valid = Vec::new();
    for (i, decl) in decls.iter().enumerate() {
        let words = if decl.path.is_empty() {
            0
        } else {
            decl.path.split(' ').count()
        };
        if words == 0 || words > limits.command_path_words {
            let limit = if words == 0 {
                Bound::Between {
                    min: 1,
                    max: limits.command_path_words,
                }
            } else {
                Bound::AtMost(limits.command_path_words)
            };
            out.push(Violation::new(
                RuleId::CMD_10,
                decl.path,
                limit,
                Actual::Count(words),
            ));
        } else {
            valid.push(i);
        }
    }
    valid
}

impl<'d, 'a> Tree<'d, 'a> {
    /// Builds the nodes from the `valid` handler paths, attaches every valid
    /// declaration to its node and reports REG-04 duplicates and orphans.
    fn build(
        decls: &'d [CommandDecl<'a>],
        valid: &[usize],
        limits: &'d Limits,
        out: &mut Vec<Violation>,
    ) -> Self {
        let mut tree = Tree {
            decls,
            nodes: Vec::new(),
            roots: Vec::new(),
            limits,
        };
        let mut by_path: HashMap<&'a str, usize> = HashMap::new();
        for &i in valid {
            if matches!(decls[i].kind, DeclKind::Handler { .. }) {
                tree.insert(i, &mut by_path);
            }
        }
        let mut orphans: Vec<(&str, Vec<&str>)> = Vec::new();
        for &i in valid {
            let decl = &decls[i];
            match by_path.get(decl.path) {
                Some(&node) => tree.nodes[node].decls.push(i),
                None => match orphans.iter_mut().find(|(p, _)| *p == decl.path) {
                    Some((_, sources)) => sources.push(decl.source),
                    None => orphans.push((decl.path, vec![decl.source])),
                },
            }
        }
        for &i in valid {
            let decl = &decls[i];
            match by_path.get(decl.path) {
                Some(&node) if matches!(decl.kind, DeclKind::Handler { .. }) => {
                    let handlers: Vec<usize> = tree.handlers(&tree.nodes[node]).collect();
                    if handlers.len() > 1 && handlers[0] == i {
                        let mut v = Violation::new(
                            RuleId::REG_04,
                            decl.path,
                            Bound::Describe("one handler per command path"),
                            Actual::Text(decl.path.to_owned()),
                        );
                        for h in handlers {
                            v = v.with_note(format!(
                                "registered by {}",
                                source_name(decls[h].source)
                            ));
                        }
                        out.push(v);
                    }
                }
                Some(_) => {}
                None => {
                    if let Some(pos) = orphans.iter().position(|(p, _)| *p == decl.path) {
                        let (path, sources) = orphans.remove(pos);
                        let mut v = Violation::new(
                            RuleId::REG_04,
                            path,
                            Bound::Describe("metadata only for a registered command path"),
                            Actual::Text(path.to_owned()),
                        );
                        for source in sources {
                            v = v.with_note(format!("declared by {}", source_name(source)));
                        }
                        out.push(v);
                    }
                }
            }
        }
        tree
    }

    /// Adds the nodes of handler `i`'s path.
    fn insert(&mut self, i: usize, by_path: &mut HashMap<&'a str, usize>) {
        let path_str = self.decls[i].path;
        let mut parent: Option<usize> = None;
        let mut end = 0;
        for (depth, word) in path_str.split(' ').enumerate() {
            if depth > 0 {
                end += 1;
            }
            end += word.len();
            let path = &path_str[..end];
            let index = if let Some(&index) = by_path.get(path) {
                index
            } else {
                let index = self.nodes.len();
                self.nodes.push(Node {
                    path,
                    word,
                    depth: depth + 1,
                    children: Vec::new(),
                    decls: Vec::new(),
                    first_child_decl: None,
                });
                match parent {
                    Some(p) => self.nodes[p].children.push(index),
                    None => self.roots.push(index),
                }
                by_path.insert(path, index);
                index
            };
            if let Some(p) = parent {
                self.nodes[p].first_child_decl.get_or_insert(i);
            }
            parent = Some(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::expand;

    const D: LocalizedText<'static> = LocalizedText::new("d");

    fn h<'a>(source: &'a str, path: &'a str) -> CommandDecl<'a> {
        CommandDecl::handler(source, path).description(D)
    }

    fn m<'a>(source: &'a str, path: &'a str) -> CommandDecl<'a> {
        CommandDecl::meta(source, path)
    }

    fn check(decls: &[CommandDecl<'_>]) -> Vec<Violation> {
        check_with(decls, &Limits::default())
    }

    fn check_with(decls: &[CommandDecl<'_>], limits: &Limits) -> Vec<Violation> {
        match validate_tree(decls, limits) {
            Ok(()) => Vec::new(),
            Err(errs) => {
                assert!(!errs.is_empty());
                errs
            }
        }
    }

    fn rules(errs: &[Violation]) -> Vec<RuleId> {
        errs.iter().map(|v| v.rule).collect()
    }

    fn expanded(pattern: &str) -> Vec<String> {
        expand(pattern, &Limits::default()).expect("valid pattern")
    }

    #[test]
    fn cmd_10_empty_input_is_ok() {
        assert_eq!(validate_tree(&[], &Limits::default()), Ok(()));
    }

    #[test]
    fn cmd_11_ac_valid_tree() {
        let paths = expanded("config (set|get) (prefix|welcome)");
        let decls: Vec<_> = paths.iter().map(|p| h("", p)).collect();
        assert_eq!(validate_tree(&decls, &Limits::default()), Ok(()));
    }

    #[test]
    fn cmd_14_groups_and_subcommands_side_by_side() {
        assert_eq!(check(&[h("", "mod ban"), h("", "mod case show")]), []);
    }

    #[test]
    fn cmd_11_ac_optional_pattern() {
        let paths = expanded("config (set|get) (prefix|welcome)?");
        let decls: Vec<_> = paths.iter().map(|p| h("", p)).collect();
        let errs = check(&decls);
        let describe = Bound::Describe("a subcommand or a subcommand group, not both");
        assert_eq!(
            errs,
            [
                Violation::new(
                    RuleId::CMD_11,
                    "config set",
                    describe.clone(),
                    Actual::Text("set".into())
                )
                .with_note("`config set` makes `set` a subcommand")
                .with_note("`config set prefix` makes `set` a subcommand group"),
                Violation::new(
                    RuleId::CMD_11,
                    "config get",
                    describe,
                    Actual::Text("get".into())
                )
                .with_note("`config get` makes `get` a subcommand")
                .with_note("`config get prefix` makes `get` a subcommand group"),
            ]
        );
    }

    #[test]
    fn cmd_11_across_sources() {
        let errs = check(&[h("m1", "config set"), h("m2", "config set prefix")]);
        assert_eq!(rules(&errs), [RuleId::CMD_11]);
        assert_eq!(
            errs[0].notes,
            [
                "`config set` makes `set` a subcommand (from `m1`)",
                "`config set prefix` makes `set` a subcommand group (from `m2`)"
            ]
        );
    }

    #[test]
    fn cmd_12_invokable_base_with_subcommands() {
        let errs = check(&[h("a", "config"), h("b", "config show")]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_12,
                "config",
                Bound::Describe("a base command with subcommands is not invokable"),
                Actual::Text("config".into())
            )
            .with_note("`config` makes `config` invokable (from `a`)")
            .with_note("`config show` gives `config` subcommands (from `b`)")]
        );
    }

    #[test]
    fn cmd_10_path_depth() {
        let errs = check(&[h("", "a b c d")]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_10,
                "a b c d",
                Bound::AtMost(3),
                Actual::Count(4)
            )]
        );
        let errs = check(&[h("", "")]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_10,
                "",
                Bound::Between { min: 1, max: 3 },
                Actual::Count(0)
            )]
        );
        let limits = Limits {
            command_path_words: 2,
            ..Limits::default()
        };
        let errs = check_with(&[h("", "a b c")], &limits);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CMD_10,
                "a b c",
                Bound::AtMost(2),
                Actual::Count(3)
            )]
        );
    }

    #[test]
    fn cmd_03_words_checked_once_per_node() {
        let paths = expanded("Config (set|get) (prefix|welcome)?");
        let decls: Vec<_> = paths.iter().map(|p| h("", p)).collect();
        let errs = check(&decls);
        assert_eq!(
            rules(&errs),
            [RuleId::CMD_03, RuleId::CMD_11, RuleId::CMD_11]
        );
        assert_eq!(errs[0].path, "Config");
        assert_eq!(errs[0].help.as_deref(), Some("use `config`"));
    }

    #[test]
    fn cmd_02_empty_word_reported_once() {
        for decls in [
            vec![h("", "config  set")],
            vec![h("", "config ")],
            vec![h("", "config  set"), h("", "config ")],
        ] {
            let errs = check(&decls);
            let cmd_02: Vec<_> = errs.iter().filter(|v| v.rule == RuleId::CMD_02).collect();
            assert_eq!(cmd_02.len(), 1, "{decls:?}");
            assert_eq!(cmd_02[0].path, "config ");
            assert_eq!(cmd_02[0].actual, Actual::Count(0));
        }
    }

    fn cmd_04(path: &str, count: usize, note: &str) -> Violation {
        Violation::new(
            RuleId::CMD_04,
            path,
            Bound::Between { min: 1, max: 100 },
            Actual::Count(count),
        )
        .with_note(note)
    }

    #[test]
    fn cmd_04_description_lengths() {
        let x100 = "x".repeat(100);
        let x101 = "x".repeat(101);
        let e50 = "😀".repeat(50);
        let e51 = "😀".repeat(51);
        let desc = |text| CommandDecl::handler("", "a").description(LocalizedText::new(text));
        assert_eq!(
            check(&[CommandDecl::handler("", "a")]),
            [cmd_04("a", 0, "missing description")]
        );
        assert_eq!(
            check(&[desc("")]),
            [cmd_04("a", 0, "in the default description")]
        );
        assert_eq!(check(&[desc(&x100)]), []);
        assert_eq!(
            check(&[desc(&x101)]),
            [cmd_04("a", 101, "in the default description")]
        );
        assert_eq!(check(&[desc(&e50)]), []);
        assert_eq!(
            check(&[desc(&e51)]),
            [cmd_04("a", 102, "in the default description")]
        );
        let localized = [x101.as_str()];
        assert_eq!(
            check(&[CommandDecl::handler("", "a")
                .description(LocalizedText::new("ok").with_localized(&localized))]),
            [cmd_04("a", 101, "in localized description #0")]
        );
        assert_eq!(
            check(&[
                m("", "config").description(LocalizedText::new(&x101)),
                h("", "config show"),
            ]),
            [cmd_04("config", 101, "in the default description")]
        );
        // A group or base without a handler may lack a description.
        assert_eq!(check(&[h("", "config show")]), []);
    }

    #[test]
    fn cmd_13_children_and_options_per_level() {
        let names: Vec<String> = (0..26).map(|i| format!("a s{i}")).collect();
        let decls: Vec<_> = names[..25].iter().map(|p| h("", p)).collect();
        assert_eq!(check(&decls), []);
        let decls: Vec<_> = names.iter().map(|p| h("", p)).collect();
        assert_eq!(
            check(&decls),
            [Violation::new(
                RuleId::CMD_13,
                "a",
                Bound::AtMost(25),
                Actual::Count(26)
            )]
        );
        let grouped: Vec<String> = (0..26).map(|i| format!("a g s{i}")).collect();
        let decls: Vec<_> = grouped.iter().map(|p| h("", p)).collect();
        assert_eq!(
            check(&decls),
            [Violation::new(
                RuleId::CMD_13,
                "a g",
                Bound::AtMost(25),
                Actual::Count(26)
            )]
        );
        assert_eq!(check(&[h("", "a b").with_options(25, 0)]), []);
        assert_eq!(
            check(&[h("", "a b").with_options(26, 0)]),
            [Violation::new(
                RuleId::CMD_13,
                "a b",
                Bound::AtMost(25),
                Actual::Count(26)
            )]
        );
        let limits = Limits {
            command_options: 2,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&[h("", "a b"), h("", "a c"), h("", "a d")], &limits),
            [Violation::new(
                RuleId::CMD_13,
                "a",
                Bound::AtMost(2),
                Actual::Count(3)
            )]
        );
    }

    fn cmd_05(path: &str, max: usize, count: usize) -> Violation {
        Violation::new(
            RuleId::CMD_05,
            path,
            Bound::AtMost(max),
            Actual::Count(count),
        )
        .with_note("longest variant per field")
    }

    #[test]
    fn cmd_05_boundary() {
        let x100 = "x".repeat(100);
        let a = CommandDecl::handler("", "a").description(LocalizedText::new(&x100));
        assert_eq!(check(&[a.with_options(0, 7899)]), []);
        assert_eq!(check(&[a.with_options(0, 7900)]), [cmd_05("a", 8000, 8001)]);
    }

    #[test]
    fn cmd_05_longest_variant_counts() {
        let y100 = "y".repeat(100);
        let localized = [y100.as_str()];
        let a = CommandDecl::handler("", "a")
            .description(LocalizedText::new("x").with_localized(&localized))
            .with_options(0, 7899);
        assert_eq!(check(&[a]), []);
        let names = ["abcdefghij"];
        assert_eq!(
            check(&[a, m("", "a").name_localized(&names)]),
            [cmd_05("a", 8000, 8009)]
        );
    }

    #[test]
    fn cmd_05_sums_the_subtree() {
        let limits = Limits {
            command_size: 4,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&[h("", "a b"), h("", "a c")], &limits),
            [cmd_05("a", 4, 5)]
        );
    }

    #[test]
    fn reg_04_duplicate_handler_path() {
        let errs = check(&[h("m1", "a b"), h("m2", "a b"), h("", "a b")]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::REG_04,
                "a b",
                Bound::Describe("one handler per command path"),
                Actual::Text("a b".into())
            )
            .with_note("registered by `m1`")
            .with_note("registered by `m2`")
            .with_note("registered by (unnamed)")]
        );
    }

    #[test]
    fn reg_04_duplicate_budget_uses_the_first() {
        let limits = Limits {
            command_size: 10,
            ..Limits::default()
        };
        let errs = check_with(
            &[h("m1", "a b"), h("m2", "a b").with_options(0, 100)],
            &limits,
        );
        assert_eq!(rules(&errs), [RuleId::REG_04]);
    }

    #[test]
    fn reg_04_conflicting_descriptions() {
        let errs = check(&[
            m("m1", "a").description(LocalizedText::new("x")),
            m("m2", "a").description(LocalizedText::new("y")),
            h("m1", "a b"),
        ]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::REG_04,
                "a",
                Bound::Describe("one description per command or group"),
                Actual::Text("a".into())
            )
            .with_note("`m1` declares `x`")
            .with_note("`m2` declares `y`")]
        );
        let localized = ["z"];
        let same = LocalizedText::new("x").with_localized(&localized);
        assert_eq!(
            check(&[
                m("m1", "a").description(same),
                m("m2", "a").description(same),
                h("m1", "a b"),
            ]),
            []
        );
        let other = LocalizedText::new("x").with_localized(&["w"]);
        assert_eq!(
            rules(&check(&[
                m("m1", "a").description(same),
                m("m2", "a").description(other),
                h("m1", "a b"),
            ])),
            [RuleId::REG_04]
        );
    }

    #[test]
    fn reg_04_metadata_without_handler() {
        let errs = check(&[m("m1", "zzz"), h("", "a")]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::REG_04,
                "zzz",
                Bound::Describe("metadata only for a registered command path"),
                Actual::Text("zzz".into())
            )
            .with_note("declared by `m1`")]
        );
    }

    #[test]
    fn reg_04_orphan_groups_sources_per_path() {
        let errs = check(&[m("m1", "zzz"), m("m2", "zzz")]);
        assert_eq!(rules(&errs), [RuleId::REG_04]);
        assert_eq!(errs[0].notes, ["declared by `m1`", "declared by `m2`"]);
    }

    #[test]
    fn reg_04_orphans_and_duplicates_in_first_occurrence_order() {
        let errs = check(&[m("m1", "zzz"), h("", "a b"), h("", "a b"), m("m2", "yyy")]);
        assert!(errs.iter().all(|v| v.rule == RuleId::REG_04));
        let paths: Vec<&str> = errs.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(paths, ["zzz", "a b", "yyy"]);
    }

    #[test]
    fn cmd_10_cmd_03_cmd_13_reported_at_once() {
        let children: Vec<String> = (0..26).map(|i| format!("base c{i}")).collect();
        let mut decls = vec![h("", "a b c d"), h("", "Ab x")];
        decls.extend(children.iter().map(|p| h("", p)));
        assert_eq!(
            rules(&check(&decls)),
            [RuleId::CMD_10, RuleId::CMD_03, RuleId::CMD_13]
        );
    }

    #[test]
    fn reg_04_order_puts_registration_conflicts_before_node_rules() {
        let errs = check(&[
            m("m1", "a").description(LocalizedText::new("x")),
            m("m2", "a").description(LocalizedText::new("y")),
            h("", "a b"),
            h("", "a b"),
            h("", "A"),
            h("", "a b c d"),
        ]);
        assert_eq!(
            rules(&errs),
            [
                RuleId::CMD_10,
                RuleId::REG_04,
                RuleId::CMD_03,
                RuleId::REG_04
            ]
        );
    }
}
