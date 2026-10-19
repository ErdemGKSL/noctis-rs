//! Command Pattern expansion and chat-input command tree rules (CMD-01..05,
//! CMD-10..16, REG-01..04).
//!
//! The `#[command]` macro and the App-build merge both call these functions, so
//! the two can never disagree. Every function takes [`Limits`](crate::Limits),
//! reads only its fields and reports every violation at once.
//!
//! # Command Patterns
//!
//! A pattern is a list of tokens separated by Unicode whitespace. A token is
//! either a plain word or a group `(a|b|…)`, optionally followed directly by `?`.
//! A group is a whole token, its alternatives are non-empty single words, and
//! groups do not nest; anything else is a CMD-16 syntax error, reported with its
//! byte offset ([`expand`] stops at the first one).
//!
//! [`expand`] yields the Command Paths in odometer order: the leftmost group
//! varies slowest, alternatives come in declaration order, and an optional
//! group's "absent" choice comes after its alternatives. Words are joined with
//! one ASCII space. The expansion count is a saturating product checked against
//! `pattern_expansions` before any path is built (CMD-15); identical expansions
//! are CMD-15 too.
//!
//! ```
//! use noctis_rules::Limits;
//! use noctis_rules::command::expand;
//!
//! let paths = expand("config (set|get) (prefix|welcome)?", &Limits::default()).unwrap();
//! assert_eq!(
//!     paths,
//!     [
//!         "config set prefix", "config set welcome", "config set",
//!         "config get prefix", "config get welcome", "config get",
//!     ]
//! );
//! ```
//!
//! # Trees
//!
//! [`validate_tree`] checks one set of [`CommandDecl`]s. Paths are split on
//! single ASCII spaces, so `"config  set"` has an empty middle word. Every word
//! prefix of a handler path is a node: 1 word is a base command, 2 a subcommand
//! or group, 3 a subcommand inside a group. Meta declarations add a description
//! or localized names to an existing node.
//!
//! Violations come in this order: per-declaration CMD-10 (input order); REG-04
//! duplicate handler paths and metadata for unregistered paths (first
//! occurrence); then every node in first-appearance pre-order with its name
//! rules (CMD-01..03), CMD-04, CMD-11/CMD-12 and CMD-13 (children, then
//! options), and CMD-05 for each base after its subtree; last, REG-04
//! description conflicts.
//!
//! [`validate_merge`] groups declarations by [`Scope`] (first appearance,
//! commands before menus) and reports per scope the tree violations (with a
//! `scope: …` note), the REG-01/REG-02 counts and REG-03 context-menu name
//! clashes.

mod merge;
mod name;
mod pattern;
mod tree;

pub use merge::validate_merge;
pub use name::validate_name;
pub use pattern::expand;
pub use tree::validate_tree;

use crate::len::discord_len;

/// A text with its default value and localized variants.
///
/// Size budgets (CMD-05) count only the longest variant, [`LocalizedText::longest`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalizedText<'a> {
    /// The default-locale text.
    pub default: &'a str,
    /// The localized variants (their locales do not matter here).
    pub localized: &'a [&'a str],
}

impl<'a> LocalizedText<'a> {
    /// A text without localized variants.
    pub const fn new(default: &'a str) -> Self {
        Self {
            default,
            localized: &[],
        }
    }

    /// Sets the localized variants.
    #[must_use]
    pub const fn with_localized(self, localized: &'a [&'a str]) -> Self {
        Self { localized, ..self }
    }

    /// The largest [`discord_len`] of the default and every localized variant.
    pub fn longest(&self) -> usize {
        self.localized
            .iter()
            .map(|text| discord_len(text))
            .fold(discord_len(self.default), usize::max)
    }
}

/// Where a command is registered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// A global command.
    #[default]
    Global,
    /// A command registered in one guild (by guild id).
    Guild(u64),
}

/// What a [`CommandDecl`] contributes to its node.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclKind {
    /// An invokable command path with a handler.
    Handler {
        /// Number of options of the handler (CMD-13).
        options: usize,
        /// The caller-computed CMD-05 contribution of the handler's options:
        /// option names, descriptions, choice names and values, the longest
        /// localized variant of each.
        options_size: usize,
    },
    /// Metadata (description, localized names) for a node that handlers create.
    Meta,
}

/// One chat-input command declaration: a handler path or metadata for a node.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandDecl<'a> {
    /// The Module (or other origin) that declared it; `""` when unnamed.
    pub source: &'a str,
    /// The Command Path, words separated by single ASCII spaces.
    pub path: &'a str,
    /// The registration scope (used by [`validate_merge`]).
    pub scope: Scope,
    /// Handler or metadata.
    pub kind: DeclKind,
    /// The description of the node at `path`.
    pub description: Option<LocalizedText<'a>>,
    /// Localized variants of the last word of `path` (counted by CMD-05).
    pub name_localized: &'a [&'a str],
}

impl<'a> CommandDecl<'a> {
    /// A handler at `path` with no options, global, without description.
    pub const fn handler(source: &'a str, path: &'a str) -> Self {
        Self {
            source,
            path,
            scope: Scope::Global,
            kind: DeclKind::Handler {
                options: 0,
                options_size: 0,
            },
            description: None,
            name_localized: &[],
        }
    }

    /// Metadata for the node at `path`, global, without description.
    pub const fn meta(source: &'a str, path: &'a str) -> Self {
        Self {
            kind: DeclKind::Meta,
            ..Self::handler(source, path)
        }
    }

    /// Sets the description.
    #[must_use]
    pub const fn description(self, description: LocalizedText<'a>) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    /// Sets the localized names of the last path word.
    #[must_use]
    pub const fn name_localized(self, names: &'a [&'a str]) -> Self {
        Self {
            name_localized: names,
            ..self
        }
    }

    /// Sets the scope.
    #[must_use]
    pub const fn scope(self, scope: Scope) -> Self {
        Self { scope, ..self }
    }

    /// Makes this a handler with `count` options whose CMD-05 contribution is
    /// `size`.
    #[must_use]
    pub const fn with_options(self, count: usize, size: usize) -> Self {
        Self {
            kind: DeclKind::Handler {
                options: count,
                options_size: size,
            },
            ..self
        }
    }
}

/// The type of a context-menu command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuKind {
    /// A `USER` command.
    User,
    /// A `MESSAGE` command.
    Message,
}

/// One context-menu command declaration.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMenuDecl<'a> {
    /// The Module (or other origin) that declared it; `""` when unnamed.
    pub source: &'a str,
    /// The registration scope.
    pub scope: Scope,
    /// User or message command.
    pub kind: MenuKind,
    /// The command name (compared byte for byte).
    pub name: &'a str,
}

impl<'a> ContextMenuDecl<'a> {
    /// A global context-menu command.
    pub const fn new(source: &'a str, kind: MenuKind, name: &'a str) -> Self {
        Self {
            source,
            scope: Scope::Global,
            kind,
            name,
        }
    }

    /// Sets the scope.
    #[must_use]
    pub const fn scope(self, scope: Scope) -> Self {
        Self { scope, ..self }
    }
}

/// `" (from `src`)"`, or nothing for an unnamed source.
fn from_suffix(source: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        format!(" (from `{source}`)")
    }
}

/// `` `src` `` or `(unnamed)`.
fn source_name(source: &str) -> String {
    if source.is_empty() {
        "(unnamed)".to_owned()
    } else {
        format!("`{source}`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_05_longest_counts_the_longest_variant() {
        assert_eq!(LocalizedText::new("abc").longest(), 3);
        assert_eq!(
            LocalizedText::new("a")
                .with_localized(&["😀😀", "xyz"])
                .longest(),
            4
        );
    }

    #[test]
    fn reg_02_scope_defaults_to_global() {
        assert_eq!(Scope::default(), Scope::Global);
        let decl = CommandDecl::handler("m", "a").scope(Scope::Guild(1));
        assert_eq!(decl.scope, Scope::Guild(1));
        assert_eq!(
            ContextMenuDecl::new("m", MenuKind::User, "X").scope,
            Scope::Global
        );
    }

    #[test]
    fn cmd_13_setters_set_the_kind() {
        let decl = CommandDecl::meta("m", "a").with_options(2, 7);
        assert_eq!(
            decl.kind,
            DeclKind::Handler {
                options: 2,
                options_size: 7
            }
        );
        assert_eq!(CommandDecl::meta("m", "a").kind, DeclKind::Meta);
    }
}
