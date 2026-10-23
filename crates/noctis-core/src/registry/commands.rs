//! The merged command Registry: every Module's `#[command]`, context-menu and
//! `base`/`group` declarations, lowered into `noctis_rules` declarations,
//! checked once with `validate_merge` and folded into per-scope trees (FR-9,
//! FR-56, FR-57, AD-4, AD-7, AD-8), with their localizations from the merged
//! locale catalogs ([`localize`]; FR-41, AD-16).

mod localize;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use noctis_i18n::AppStrings;
use noctis_rules::Limits;
use noctis_rules::command::{
    Choice, CommandDecl, CommandMeta, ContextMenuDecl, DeclKind, LocalizedField, LocalizedText,
    MenuKind, OptionDecl, PathKind, Scope, measure_options, validate_merge,
};
use twilight_model::guild::Permissions;

use crate::BuildViolation;
use crate::command::CommandHandler;
use crate::menu::ContextMenuHandler;
use crate::metadata::CommandMetadata;
use crate::middleware::Chain;
use crate::module::{Module, NodeDecl};

pub(crate) use self::localize::{NodeTexts, OptionTexts, Variants};
use self::localize::{Problems, Resolver};

/// One folded chat-input handler.
pub(crate) struct CommandEntry {
    /// The id of the Module it was added to.
    pub(crate) module: &'static str,
    /// The ids of that Module's ancestors, root first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the Module path, kept for diagnostics; Middleware chains are folded beside it and tests read it"
        )
    )]
    pub(crate) ancestors: Arc<[&'static str]>,
    /// The Middleware chain of that Module.
    pub(crate) chain: Chain,
    pub(crate) handler: CommandHandler,
}

/// One folded context-menu handler.
pub(crate) struct MenuEntry {
    /// The id of the Module it was added to.
    pub(crate) module: &'static str,
    /// The ids of that Module's ancestors, root first.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the Module path, kept for diagnostics; Middleware chains are folded beside it and tests read it"
        )
    )]
    pub(crate) ancestors: Arc<[&'static str]>,
    /// The Middleware chain of that Module.
    pub(crate) chain: Chain,
    pub(crate) handler: ContextMenuHandler,
    /// The menu name's localizations (`ctxcmd.<key>.name`).
    pub(crate) name_localizations: Variants,
}

/// One node of a merged chat-input tree: a base, a subcommand, a group or a
/// subcommand in a group.
pub(crate) struct CommandNode {
    /// The node's last word.
    pub(crate) word: &'static str,
    /// The node's full Command Path.
    pub(crate) path: &'static str,
    /// The resolved description (declared, from the default-locale file,
    /// the handler's, or the fallback).
    pub(crate) description: &'static str,
    /// The localizations of the node's name and description.
    pub(crate) texts: NodeTexts,
    /// The localizations of the handler's options, parallel to its
    /// `options()`; empty without a handler.
    pub(crate) options: Vec<OptionTexts>,
    /// The resolved base metadata (guilds empty); unset below a base.
    pub(crate) metadata: CommandMetadata,
    /// The handler at exactly this path, if any.
    pub(crate) handler: Option<Arc<CommandEntry>>,
    /// The children, in first appearance.
    pub(crate) children: Vec<CommandNode>,
}

/// Every command registered in one scope.
pub(crate) struct ScopeCommands {
    pub(crate) scope: Scope,
    /// The bases, in first appearance.
    pub(crate) bases: Vec<CommandNode>,
    /// The handlers, keyed by full Command Path.
    paths: HashMap<&'static str, Arc<CommandEntry>>,
    /// The context menus, keyed by type, then name.
    menus: HashMap<MenuKind, HashMap<&'static str, Arc<MenuEntry>>>,
    /// The context menus, in input order.
    menu_order: Vec<Arc<MenuEntry>>,
}

impl ScopeCommands {
    /// The handler registered at `path`, in O(1).
    pub(crate) fn command(&self, path: &str) -> Option<&Arc<CommandEntry>> {
        self.paths.get(path)
    }

    /// The context menu of `kind` named `name`, in O(1).
    pub(crate) fn menu(&self, kind: MenuKind, name: &str) -> Option<&Arc<MenuEntry>> {
        self.menus.get(&kind)?.get(name)
    }

    /// The context menus, in input order (Publish JSON order).
    pub(crate) fn menus(&self) -> impl Iterator<Item = &Arc<MenuEntry>> {
        self.menu_order.iter()
    }
}

/// The merged, immutable command Registry: one entry per scope, in first
/// appearance (commands, then menus). Empty when the checks failed.
#[derive(Default)]
pub(crate) struct Commands {
    scopes: Vec<ScopeCommands>,
}

/// One Module in fold order, with its ancestors.
pub(super) struct Folded<'m> {
    pub(super) module: &'m Module,
    pub(super) ancestors: Arc<[&'static str]>,
    /// The Module's Middleware chain.
    pub(super) chain: Chain,
}

/// A description fallback: the node, its scope and where the description
/// comes from.
struct Fallback {
    scope: Scope,
    node: &'static str,
    from_path: &'static str,
    from_module: &'static str,
}

/// The declarations handed to `validate_merge`, with their origins.
#[derive(Default)]
struct Lowered {
    commands: Vec<CommandDecl<'static>>,
    /// Parallel to `commands`: the handler a handler declaration lowers.
    entries: Vec<Option<Arc<CommandEntry>>>,
    menus: Vec<ContextMenuDecl<'static>>,
    /// Parallel to `menus`.
    menu_entries: Vec<Arc<MenuEntry>>,
}

impl Lowered {
    fn push(&mut self, decl: CommandDecl<'static>, entry: Option<Arc<CommandEntry>>) {
        self.commands.push(decl);
        self.entries.push(entry);
    }
}

/// Whether `node` is `path` or a word prefix of it.
fn is_word_prefix(node: &str, path: &str) -> bool {
    path.strip_prefix(node)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

/// Every strict word prefix of `path` (`"a b c"` gives `"a"`, `"a b"`).
fn strict_prefixes(path: &'static str) -> impl Iterator<Item = &'static str> {
    path.match_indices(' ').map(move |(at, _)| &path[..at])
}

/// `scope: global` or `scope: guild <id>`.
fn scope_text(scope: Scope) -> String {
    match scope {
        Scope::Global => "scope: global".to_owned(),
        Scope::Guild(id) => format!("scope: guild {id}"),
    }
}

impl Commands {
    /// Lowers the commands, menus and node declarations of `modules` (fold
    /// order), resolves their localizations against `strings`, runs
    /// `validate_merge` (whose CMD-05 budget counts the longest valid
    /// variant per field, LOC-05) and the LOC-02 / LOC-03 checks and, when
    /// both pass, logs the description fallbacks and builds the Registry.
    /// Otherwise the Registry is empty and every violation is returned:
    /// each [`BuildViolation::Command`] in `validate_merge` order, then each
    /// [`BuildViolation::CommandLocalization`] (sorted by key, locale, rule).
    pub(super) fn fold(
        modules: &[Folded<'_>],
        strings: Option<&AppStrings>,
    ) -> (Self, Vec<BuildViolation>) {
        let limits = Limits::default();
        let resolver = Resolver::new(strings);
        let mut lowered = lower(modules, resolver);
        let fallbacks = add_fallbacks(&mut lowered, resolver);
        let table = Table::new(&lowered, resolver);
        let localization = table.check(&lowered, &limits).finish(resolver);
        let merged = {
            let budget = table.budget(&limits);
            let decls = table.decls(&lowered, &budget, &limits);
            validate_merge(&decls, &lowered.menus, &limits)
        };
        if let Err(violations) = merged {
            let mut out: Vec<BuildViolation> = violations
                .into_iter()
                .map(BuildViolation::Command)
                .collect();
            out.extend(localization);
            return (Self::default(), out);
        }
        if !localization.is_empty() {
            return (Self::default(), localization);
        }
        for fallback in &fallbacks {
            tracing::warn!(
                target: "noctis::registry",
                "no description declared for `{}` ({}): using the description of `{}` from \
                 Module `{}`; declare one with `Module::base` or `Module::group`",
                fallback.node,
                scope_text(fallback.scope),
                fallback.from_path,
                fallback.from_module,
            );
        }
        (Self::build(&lowered, &table), Vec::new())
    }

    /// Every scope, in first appearance (commands, then menus).
    pub(crate) fn scopes(&self) -> &[ScopeCommands] {
        &self.scopes
    }

    /// The commands of `scope`, if any are registered there.
    pub(crate) fn scope(&self, scope: Scope) -> Option<&ScopeCommands> {
        self.scopes.iter().find(|s| s.scope == scope)
    }

    /// Builds the per-scope trees and maps from checked declarations.
    fn build(lowered: &Lowered, table: &Table) -> Self {
        let mut order: Vec<Scope> = Vec::new();
        for scope in lowered
            .commands
            .iter()
            .map(|c| c.scope)
            .chain(lowered.menus.iter().map(|m| m.scope))
        {
            if !order.contains(&scope) {
                order.push(scope);
            }
        }
        let scopes = order
            .into_iter()
            .map(|scope| build_scope(lowered, table, scope))
            .collect();
        Self { scopes }
    }

    /// The `__testing::command_tree` rendering.
    pub(crate) fn render(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for scope in &self.scopes {
            lines.push(match scope.scope {
                Scope::Global => "global".to_owned(),
                Scope::Guild(id) => format!("guild {id}"),
            });
            for base in &scope.bases {
                render_node(base, 1, &mut lines);
            }
            for menu in &scope.menu_order {
                let kind = match menu.handler.kind() {
                    MenuKind::User => "user",
                    MenuKind::Message => "message",
                };
                lines.push(format!(
                    "  {kind} \"{}\"{} -> {} ({})",
                    menu.handler.menu_name(),
                    metadata_suffix(menu.handler.metadata()),
                    menu.handler.name(),
                    menu.module,
                ));
            }
        }
        lines.join("\n")
    }
}

/// Lowers every Module in fold order: its commands (one declaration per path
/// and scope, in `add` order), its menus (one per scope), then its node
/// declarations (in call order, one per scope its node exists in).
fn lower(modules: &[Folded<'_>], resolver: Resolver<'_>) -> Lowered {
    // Node declarations follow the scopes of their node, which any Module's
    // handlers may create.
    let mut handler_scopes: Vec<(&'static str, Scope)> = Vec::new();
    for folded in modules {
        for handler in folded.module.commands() {
            let scopes = handler.metadata().scopes();
            for &path in handler.paths() {
                handler_scopes.extend(scopes.iter().map(|&scope| (path, scope)));
            }
        }
    }

    let mut lowered = Lowered::default();
    for folded in modules {
        let module = folded.module.id();
        for handler in folded.module.commands() {
            let entry = Arc::new(CommandEntry {
                module,
                ancestors: Arc::clone(&folded.ancestors),
                chain: folded.chain.clone(),
                handler: handler.clone(),
            });
            let options = handler.options();
            let options_size = measure_options(options, &Limits::default());
            let metadata = handler.metadata();
            for (&path, &description) in handler.paths().iter().zip(handler.descriptions()) {
                for scope in metadata.scopes() {
                    let decl = CommandDecl::handler(module, path)
                        .description(LocalizedText::new(description))
                        .with_options(options.len(), options_size)
                        .metadata(metadata.to_rules())
                        .scope(scope);
                    lowered.push(decl, Some(Arc::clone(&entry)));
                }
            }
        }
        for handler in folded.module.menus() {
            let entry = Arc::new(MenuEntry {
                module,
                ancestors: Arc::clone(&folded.ancestors),
                chain: folded.chain.clone(),
                handler: handler.clone(),
                name_localizations: resolver.menu(handler.key()),
            });
            for scope in handler.metadata().scopes() {
                lowered.menus.push(
                    ContextMenuDecl::new(module, handler.kind(), handler.menu_name())
                        .metadata(handler.metadata().to_rules())
                        .scope(scope),
                );
                lowered.menu_entries.push(Arc::clone(&entry));
            }
        }
        for node in folded.module.nodes() {
            let decl = node_decl(module, *node);
            let mut scopes: Vec<Scope> = Vec::new();
            for &(handler_path, scope) in &handler_scopes {
                if is_word_prefix(decl.path, handler_path) && !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
            if scopes.is_empty() {
                // Reported as metadata for an unregistered path (REG-04).
                scopes.push(Scope::Global);
            }
            for scope in scopes {
                lowered.push(decl.scope(scope), None);
            }
        }
    }
    lowered
}

/// The global meta declaration of one `base`/`group` call.
fn node_decl(module: &'static str, node: NodeDecl) -> CommandDecl<'static> {
    match node {
        NodeDecl::Base { name, meta } => {
            let decl = CommandDecl::meta(module, name).metadata(meta.to_rules());
            match meta.description {
                Some(text) => decl.description(LocalizedText::new(text)),
                None => decl,
            }
        }
        NodeDecl::Group { path, description } => {
            CommandDecl::meta(module, path).description(LocalizedText::new(description))
        }
    }
}

/// Appends one synthetic description per undescribed node (FR-57, AD-7) and
/// returns the handler fallbacks in order: per scope (first appearance),
/// per node (first appearance). A node whose `cmd.<path>.description` is in
/// its owner catalog's default-locale file takes that text, declared by the
/// catalog's origin and without a warning.
fn add_fallbacks(lowered: &mut Lowered, resolver: Resolver<'_>) -> Vec<Fallback> {
    let mut scopes: Vec<Scope> = Vec::new();
    for decl in &lowered.commands {
        if !scopes.contains(&decl.scope) {
            scopes.push(decl.scope);
        }
    }
    let is_handler = |d: &CommandDecl<'_>| matches!(d.kind, DeclKind::Handler { .. });
    let mut fallbacks = Vec::new();
    let mut synthetic = Vec::new();
    for scope in scopes {
        let in_scope: Vec<&CommandDecl<'static>> = lowered
            .commands
            .iter()
            .filter(|d| d.scope == scope)
            .collect();
        let leaves: Vec<&CommandDecl<'static>> = in_scope
            .iter()
            .copied()
            .filter(|d| is_handler(d) && PathKind::of(d.path).is_some())
            .collect();
        let mut nodes: Vec<&'static str> = Vec::new();
        for leaf in &leaves {
            for prefix in strict_prefixes(leaf.path) {
                if !nodes.contains(&prefix) {
                    nodes.push(prefix);
                }
            }
        }
        for node in nodes {
            let described = in_scope
                .iter()
                .any(|d| d.path == node && (is_handler(d) || d.description.is_some()));
            if described {
                continue;
            }
            if let Some((text, origin)) = resolver.default_description(node) {
                synthetic.push(
                    CommandDecl::meta(origin, node)
                        .description(LocalizedText::new(text))
                        .scope(scope),
                );
                continue;
            }
            let below = |d: &&&CommandDecl<'static>| {
                d.path.len() > node.len() && is_word_prefix(node, d.path)
            };
            // `min_by_key` keeps the first of equal keys: fold order breaks ties.
            let Some(from) = leaves
                .iter()
                .filter(below)
                .min_by_key(|d| (d.source, d.path))
            else {
                continue;
            };
            let Some(description) = from.description else {
                continue;
            };
            synthetic.push(
                CommandDecl::meta(from.source, node)
                    .description(description)
                    .scope(scope),
            );
            fallbacks.push(Fallback {
                scope,
                node,
                from_path: from.path,
                from_module: from.source,
            });
        }
    }
    for decl in synthetic {
        lowered.push(decl, None);
    }
    fallbacks
}

/// The localizations of every registered node and handler declaration.
struct Table {
    /// Per node path (every handler path and its strict prefixes).
    nodes: HashMap<&'static str, NodeTexts>,
    /// The node paths, in first appearance (the check order).
    node_order: Vec<&'static str>,
    /// Parallel to `Lowered::commands`: the option localizations of a
    /// handler declaration.
    options: Vec<Option<Vec<OptionTexts>>>,
}

/// The LOC-02-valid localized names and descriptions of one node, which
/// the CMD-05 budget counts.
struct Budget {
    names: Vec<&'static str>,
    descriptions: Vec<&'static str>,
}

impl Table {
    fn new(lowered: &Lowered, resolver: Resolver<'_>) -> Self {
        let mut nodes: HashMap<&'static str, NodeTexts> = HashMap::new();
        let mut node_order = Vec::new();
        let mut options = Vec::with_capacity(lowered.commands.len());
        for (decl, entry) in lowered.commands.iter().zip(&lowered.entries) {
            let Some(entry) = entry else {
                options.push(None);
                continue;
            };
            for node in strict_prefixes(decl.path).chain([decl.path]) {
                if !nodes.contains_key(node) {
                    nodes.insert(node, resolver.node(node));
                    node_order.push(node);
                }
            }
            options.push(Some(resolver.options(decl.path, entry.handler.options())));
        }
        Self {
            nodes,
            node_order,
            options,
        }
    }

    /// LOC-02 over every value; LOC-03 over every options array and, per
    /// scope, every children array.
    fn check(&self, lowered: &Lowered, limits: &Limits) -> Problems {
        let mut problems = Problems::default();
        for path in &self.node_order {
            let texts = &self.nodes[path];
            problems.values(LocalizedField::Name, &texts.name, limits);
            problems.values(LocalizedField::Description, &texts.description, limits);
        }
        for (texts, entry) in self.options.iter().zip(&lowered.entries) {
            let (Some(texts), Some(entry)) = (texts, entry) else {
                continue;
            };
            for option in texts {
                problems.values(LocalizedField::Name, &option.name, limits);
                problems.values(LocalizedField::Description, &option.description, limits);
                for choice in &option.choices {
                    problems.values(LocalizedField::ChoiceName, choice, limits);
                }
            }
            let array: Vec<(&str, &Variants)> = entry
                .handler
                .options()
                .iter()
                .zip(texts)
                .map(|(decl, texts)| (decl.name, &texts.name))
                .collect();
            problems.names(&array);
        }
        for children in children_arrays(lowered) {
            let array: Vec<(&str, &Variants)> = children
                .iter()
                .filter_map(|child| {
                    let word = child.rsplit(' ').next().unwrap_or_default();
                    self.nodes.get(child).map(|texts| (word, &texts.name))
                })
                .collect();
            problems.names(&array);
        }
        for entry in &lowered.menu_entries {
            problems.values(
                LocalizedField::ContextMenuName,
                &entry.name_localizations,
                limits,
            );
        }
        problems
    }

    /// The per-node lists the CMD-05 budget counts.
    fn budget(&self, limits: &Limits) -> HashMap<&'static str, Budget> {
        self.nodes
            .iter()
            .map(|(&path, texts)| {
                (
                    path,
                    Budget {
                        names: texts.name.valid_values(LocalizedField::Name, limits),
                        descriptions: texts
                            .description
                            .valid_values(LocalizedField::Description, limits),
                    },
                )
            })
            .collect()
    }

    /// The declarations `validate_merge` checks: the lowered ones with
    /// their localized variants (LOC-05).
    fn decls<'t>(
        &self,
        lowered: &Lowered,
        budget: &'t HashMap<&'static str, Budget>,
        limits: &Limits,
    ) -> Vec<CommandDecl<'t>> {
        lowered
            .commands
            .iter()
            .zip(&lowered.entries)
            .zip(&self.options)
            .map(|((&decl, entry), texts)| {
                let mut decl: CommandDecl<'t> = decl;
                if let Some(lists) = budget.get(decl.path) {
                    decl = decl.name_localized(&lists.names);
                    if let Some(description) = decl.description {
                        decl = decl.description(description.with_localized(&lists.descriptions));
                    }
                }
                if let (Some(entry), Some(texts), DeclKind::Handler { options, .. }) =
                    (entry, texts, decl.kind)
                {
                    let size = measure_localized(entry.handler.options(), texts, limits);
                    decl = decl.with_options(options, size);
                }
                decl
            })
            .collect()
    }
}

/// The CMD-05 share of `options` with their LOC-02-valid localized names,
/// descriptions and choice names, over loop-local copies.
fn measure_localized(options: &[OptionDecl<'_>], texts: &[OptionTexts], limits: &Limits) -> usize {
    let names: Vec<Vec<&'static str>> = texts
        .iter()
        .map(|t| t.name.valid_values(LocalizedField::Name, limits))
        .collect();
    let descriptions: Vec<Vec<&'static str>> = texts
        .iter()
        .map(|t| {
            t.description
                .valid_values(LocalizedField::Description, limits)
        })
        .collect();
    let choice_names: Vec<Vec<Vec<&'static str>>> = texts
        .iter()
        .map(|t| {
            t.choices
                .iter()
                .map(|c| c.valid_values(LocalizedField::ChoiceName, limits))
                .collect()
        })
        .collect();
    let choices: Vec<Vec<Choice<'_>>> = options
        .iter()
        .zip(&choice_names)
        .map(|(option, names)| {
            option
                .choices
                .iter()
                .zip(names)
                .map(|(choice, names)| choice.name_localized(names))
                .collect()
        })
        .collect();
    let copies: Vec<OptionDecl<'_>> = options
        .iter()
        .zip(&names)
        .zip(&descriptions)
        .zip(&choices)
        .map(|(((option, names), descriptions), choices)| {
            option
                .name_localized(names)
                .description_localized(descriptions)
                .choices(choices)
        })
        .collect();
    measure_options(&copies, limits)
}

/// Per scope (first appearance) and parent node (first appearance), the
/// paths of the parent's children (first appearance).
fn children_arrays(lowered: &Lowered) -> Vec<Vec<&'static str>> {
    let mut arrays: Vec<(Scope, &'static str, Vec<&'static str>)> = Vec::new();
    for (decl, entry) in lowered.commands.iter().zip(&lowered.entries) {
        if entry.is_none() {
            continue;
        }
        let path = decl.path;
        let mut parent: Option<&'static str> = None;
        for node in strict_prefixes(path).chain([path]) {
            if let Some(parent) = parent {
                let found = arrays
                    .iter()
                    .position(|(s, p, _)| *s == decl.scope && *p == parent);
                let at = found.unwrap_or_else(|| {
                    arrays.push((decl.scope, parent, Vec::new()));
                    arrays.len() - 1
                });
                if !arrays[at].2.contains(&node) {
                    arrays[at].2.push(node);
                }
            }
            parent = Some(node);
        }
    }
    arrays
        .into_iter()
        .map(|(_, _, children)| children)
        .collect()
}

/// The node tree, path map and menus of one checked scope.
fn build_scope(lowered: &Lowered, table: &Table, scope: Scope) -> ScopeCommands {
    let in_scope: Vec<(&CommandDecl<'static>, Option<&Arc<CommandEntry>>)> = lowered
        .commands
        .iter()
        .zip(&lowered.entries)
        .filter(|(d, _)| d.scope == scope)
        .map(|(d, e)| (d, e.as_ref()))
        .collect();

    let mut descriptions: HashMap<&'static str, &'static str> = HashMap::new();
    let mut paths: HashMap<&'static str, Arc<CommandEntry>> = HashMap::new();
    let mut options: HashMap<&'static str, &[OptionTexts]> = HashMap::new();
    for (index, (decl, entry)) in lowered.commands.iter().zip(&lowered.entries).enumerate() {
        if decl.scope != scope {
            continue;
        }
        if let Some(text) = decl.description {
            descriptions.entry(decl.path).or_insert(text.default);
        }
        if let Some(entry) = entry {
            paths.entry(decl.path).or_insert_with(|| Arc::clone(entry));
            if let Some(texts) = &table.options[index] {
                options.entry(decl.path).or_insert(texts.as_slice());
            }
        }
    }

    let level = Level {
        descriptions: &descriptions,
        handlers: &paths,
        options: &options,
        nodes: &table.nodes,
    };
    let mut bases: Vec<CommandNode> = Vec::new();
    for &(decl, entry) in &in_scope {
        if entry.is_some() {
            insert(&mut bases, decl.path, 0, &level);
        }
    }
    for base in &mut bases {
        base.metadata = base_metadata(base.path, &in_scope);
    }

    let mut menus: HashMap<MenuKind, HashMap<&'static str, Arc<MenuEntry>>> = HashMap::new();
    let mut menu_order = Vec::new();
    for (decl, entry) in lowered.menus.iter().zip(&lowered.menu_entries) {
        if decl.scope == scope {
            menus
                .entry(decl.kind)
                .or_default()
                .entry(decl.name)
                .or_insert_with(|| Arc::clone(entry));
            menu_order.push(Arc::clone(entry));
        }
    }
    ScopeCommands {
        scope,
        bases,
        paths,
        menus,
        menu_order,
    }
}

/// What [`insert`] reads per node path.
struct Level<'a> {
    descriptions: &'a HashMap<&'static str, &'static str>,
    handlers: &'a HashMap<&'static str, Arc<CommandEntry>>,
    options: &'a HashMap<&'static str, &'a [OptionTexts]>,
    nodes: &'a HashMap<&'static str, NodeTexts>,
}

/// Inserts the nodes of `path` from word `from` on (byte offset of the word
/// start) into `level`.
fn insert(level: &mut Vec<CommandNode>, path: &'static str, from: usize, by_path: &Level<'_>) {
    let end = path[from..].find(' ').map_or(path.len(), |at| from + at);
    let node_path = &path[..end];
    let word = &path[from..end];
    let index = if let Some(index) = level.iter().position(|n| n.word == word) {
        index
    } else {
        level.push(CommandNode {
            word,
            path: node_path,
            description: by_path
                .descriptions
                .get(node_path)
                .copied()
                .unwrap_or_default(),
            texts: by_path.nodes.get(node_path).cloned().unwrap_or_default(),
            options: by_path
                .options
                .get(node_path)
                .map(|texts| texts.to_vec())
                .unwrap_or_default(),
            metadata: CommandMetadata::new(),
            handler: by_path.handlers.get(node_path).cloned(),
            children: Vec::new(),
        });
        level.len() - 1
    };
    if end < path.len() {
        insert(&mut level[index].children, path, end + 1, by_path);
    }
}

/// The resolved metadata of the base at `base`: per field, the explicit base
/// declaration's value when set, else the first handler's below the base
/// (they agree once the checks pass).
fn base_metadata(
    base: &str,
    in_scope: &[(&CommandDecl<'static>, Option<&Arc<CommandEntry>>)],
) -> CommandMetadata {
    let explicit: Vec<CommandMeta<'static>> = in_scope
        .iter()
        .filter(|(d, _)| matches!(d.kind, DeclKind::Meta) && d.path == base)
        .map(|(d, _)| d.metadata)
        .collect();
    let first_handler = in_scope
        .iter()
        .find(|(d, e)| e.is_some() && is_word_prefix(base, d.path))
        .map(|(d, _)| d.metadata)
        .unwrap_or_default();
    let pick = |field: fn(&CommandMeta<'static>) -> bool| {
        explicit
            .iter()
            .copied()
            .find(&field)
            .unwrap_or(first_handler)
    };
    let mut metadata = CommandMetadata::new();
    if let Some(bits) = pick(|m| m.default_member_permissions.is_some()).default_member_permissions
    {
        metadata = metadata.default_member_permissions(Permissions::from_bits_retain(bits));
    }
    if let Some(contexts) = pick(|m| m.contexts.is_some()).contexts {
        metadata = metadata.contexts(contexts);
    }
    if let Some(types) = pick(|m| m.integration_types.is_some()).integration_types {
        metadata = metadata.integration_types(types);
    }
    if let Some(nsfw) = pick(|m| m.nsfw.is_some()).nsfw {
        metadata = metadata.nsfw(nsfw);
    }
    metadata
}

/// The set metadata fields, in field order, each with a leading space.
fn metadata_suffix(metadata: &CommandMetadata) -> String {
    let mut out = String::new();
    if let Some(permissions) = metadata.default_member_permissions {
        let _ = write!(out, " permissions={}", permissions.bits());
    }
    if let Some(contexts) = metadata.contexts {
        let names: Vec<&str> = contexts.iter().map(|c| c.name()).collect();
        let _ = write!(out, " contexts=[{}]", names.join(", "));
    }
    if let Some(types) = metadata.integration_types {
        let names: Vec<&str> = types.iter().map(|t| t.name()).collect();
        let _ = write!(out, " integration_types=[{}]", names.join(", "));
    }
    if let Some(nsfw) = metadata.nsfw {
        let _ = write!(out, " nsfw={nsfw}");
    }
    out
}

/// One node line and its children's, indented two spaces per depth.
fn render_node(node: &CommandNode, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let slash = if depth == 1 { "/" } else { "" };
    let mut line = format!(
        "{indent}{slash}{} \"{}\"{}",
        node.word,
        node.description,
        metadata_suffix(&node.metadata)
    );
    if let Some(entry) = &node.handler {
        let _ = write!(line, " -> {} ({})", entry.handler.name(), entry.module);
    }
    lines.push(line);
    for child in &node.children {
        render_node(child, depth + 1, lines);
    }
}

#[cfg(test)]
mod tests;
