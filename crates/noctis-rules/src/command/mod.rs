//! Command Pattern expansion, chat-input command tree, option, context-menu,
//! metadata and autocomplete rules (CMD-01..07, CMD-10..16, OPT-01, OPT-03..11,
//! OPT-13, CTX-01, META-01..03, REG-01..04, INT-07).
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
//! Because [`expand`] collapses whitespace, `#[command]` and the App build
//! first run [`validate_pattern_spacing`] on the pattern as written: words
//! must be separated by exactly one ASCII space, so a leading, trailing or
//! doubled space is an empty word (CMD-02) and any other whitespace is CMD-01.
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
//! # Path kinds
//!
//! A Command Path's word count decides what it registers ([`PathKind`]):
//! 1 word is a base command, 2 a subcommand of the base, 3 a subcommand in a
//! group of the base. Subcommand and group nodes exist only through paths;
//! nothing sets `required` on them (OPT-14).
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
//! options), and CMD-05 for each base after its subtree; then REG-04
//! description conflicts; last, per base in first-appearance order, the
//! metadata checks: misplaced metadata (META-02, input order), then META-01,
//! META-02 `contexts`, META-02 `integration_types` and CMD-07 conflicts.
//!
//! [`validate_merge`] groups declarations by [`Scope`] (first appearance,
//! commands before menus) and reports per scope the tree violations (with a
//! `scope: …` note), META-03 for commands then menus (input order), CTX-01 per
//! menu (input order), the REG-01/REG-02 counts and REG-03 context-menu name
//! clashes.
//!
//! # Options
//!
//! [`validate_options`] checks one options array of [`OptionDecl`]s in
//! isolation (CMD-01..04 for names and descriptions, CMD-06, CMD-13, OPT-01,
//! OPT-03..11, OPT-13); the order is on the function. [`measure_options`]
//! computes the array's CMD-05 share for [`CommandDecl::with_options`]:
//!
//! ```
//! use noctis_rules::Limits;
//! use noctis_rules::command::{
//!     CommandDecl, LocalizedText, NumericValue, OptionDecl, OptionType, measure_options,
//!     validate_options, validate_tree,
//! };
//!
//! let limits = Limits::default();
//! let options = [OptionDecl::new("days", OptionType::Integer, "How long")
//!     .required(true)
//!     .min_value(NumericValue::Int(1))
//!     .max_value(NumericValue::Int(30))];
//! assert_eq!(validate_options(&options, &limits), Ok(()));
//! let decl = CommandDecl::handler("mod", "mod ban")
//!     .description(LocalizedText::new("Ban a member"))
//!     .with_options(options.len(), measure_options(&options, &limits));
//! assert_eq!(validate_tree(&[decl], &limits), Ok(()));
//! ```
//!
//! # Autocomplete
//!
//! [`validate_autocomplete`] checks the choices an autocomplete handler
//! returns for an option of a given [`OptionType`]: more than
//! `autocomplete_choices` is INT-07, reported for the caller to truncate and
//! log as a warning in every mode; each kept choice follows OPT-09 (and OPT-03 /
//! OPT-04 for numbers); a type that cannot autocomplete is OPT-11.
//!
//! # Context menus
//!
//! [`validate_context_menu_name`] checks a context-menu name (CTX-01: length
//! only, mixed case and spaces allowed). Context-menu commands have no
//! description or options (CTX-02), and their type is only `USER` or `MESSAGE`
//! (CTX-03), by construction.
//!
//! # Metadata
//!
//! [`CommandMeta`] holds `default_member_permissions`, `contexts`,
//! `integration_types` and `nsfw`, which Discord keeps only on the base
//! command. Every [`CommandDecl`] under one base must agree (META-01, META-02,
//! CMD-07; lists compare as sets). Without an explicit base declaration (a
//! [`CommandDecl::meta`] at the 1-word path that sets the field) "unset" is a
//! value and must agree too; with one, unset handlers inherit it. Metadata on a
//! meta declaration below the base is META-02. [`validate_meta`] rejects
//! `contexts` and `integration_types` on guild-scoped declarations (META-03).

mod merge;
mod meta;
mod name;
mod option;
mod pattern;
mod tree;

pub use merge::validate_merge;
pub use meta::{CommandMeta, IntegrationType, InteractionContext, validate_meta};
pub use name::{validate_context_menu_name, validate_name};
pub use option::{
    Choice, ChoiceValue, NumericValue, OptionDecl, OptionType, measure_options,
    validate_autocomplete, validate_options,
};
pub(crate) use option::{FILE_TYPE, is_file_type};
pub use pattern::{expand, validate_pattern_spacing};
pub use tree::validate_tree;

use crate::len::discord_len;

/// What a Command Path of 1, 2 or 3 words registers (CMD-10).
///
/// ```
/// use noctis_rules::command::PathKind;
///
/// assert_eq!(PathKind::of("ping"), Some(PathKind::Command));
/// assert_eq!(PathKind::of("setup start"), Some(PathKind::Subcommand));
/// assert_eq!(PathKind::of("settings logs channel"), Some(PathKind::GroupSubcommand));
/// assert_eq!(PathKind::of("a b c d"), None);
/// assert_eq!(PathKind::GroupSubcommand.words(), 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PathKind {
    /// 1 word: a base command (`/ping`).
    Command,
    /// 2 words: a subcommand of a base (`/setup start`).
    Subcommand,
    /// 3 words: a subcommand in a subcommand group (`/settings logs channel`).
    GroupSubcommand,
}

impl PathKind {
    /// The kind of `path`, split on single ASCII spaces; `None` when it is
    /// empty, has an empty word (`"a  b"`, `" a"`) or has more than 3 words.
    pub fn of(path: &str) -> Option<Self> {
        if path.is_empty() || path.split(' ').any(str::is_empty) {
            return None;
        }
        match path.split(' ').count() {
            1 => Some(Self::Command),
            2 => Some(Self::Subcommand),
            3 => Some(Self::GroupSubcommand),
            _ => None,
        }
    }

    /// The number of words of a path of this kind (1, 2 or 3).
    #[must_use]
    pub const fn words(self) -> usize {
        match self {
            Self::Command => 1,
            Self::Subcommand => 2,
            Self::GroupSubcommand => 3,
        }
    }
}

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
    /// Base-command metadata (META-01..03, CMD-07); default: all unset.
    pub metadata: CommandMeta<'a>,
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
            metadata: CommandMeta::new(),
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

    /// Sets the base-command metadata.
    #[must_use]
    pub const fn metadata(self, metadata: CommandMeta<'a>) -> Self {
        Self { metadata, ..self }
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
///
/// Only `USER` and `MESSAGE` exist; `PRIMARY_ENTRY_POINT` is out of scope
/// (CTX-03):
///
/// ```compile_fail
/// use noctis_rules::command::MenuKind;
///
/// let _ = MenuKind::PrimaryEntryPoint; // [CTX-03]
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuKind {
    /// A `USER` command.
    User,
    /// A `MESSAGE` command.
    Message,
}

/// One context-menu command declaration.
///
/// It has no description and no options (CTX-02):
///
/// ```compile_fail
/// use noctis_rules::command::{ContextMenuDecl, MenuKind};
///
/// let _ = ContextMenuDecl::new("m", MenuKind::User, "X").description("d"); // [CTX-02]
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMenuDecl<'a> {
    /// The Module (or other origin) that declared it; `""` when unnamed.
    pub source: &'a str,
    /// The registration scope.
    pub scope: Scope,
    /// User or message command.
    pub kind: MenuKind,
    /// The command name (CTX-01; compared byte for byte by REG-03).
    pub name: &'a str,
    /// Command metadata (META-03); default: all unset.
    pub metadata: CommandMeta<'a>,
}

impl<'a> ContextMenuDecl<'a> {
    /// A global context-menu command.
    pub const fn new(source: &'a str, kind: MenuKind, name: &'a str) -> Self {
        Self {
            source,
            scope: Scope::Global,
            kind,
            name,
            metadata: CommandMeta::new(),
        }
    }

    /// Sets the scope.
    #[must_use]
    pub const fn scope(self, scope: Scope) -> Self {
        Self { scope, ..self }
    }

    /// Sets the command metadata.
    #[must_use]
    pub const fn metadata(self, metadata: CommandMeta<'a>) -> Self {
        Self { metadata, ..self }
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
    fn cmd_10_path_kind_by_word_count() {
        for (path, kind) in [
            ("a", PathKind::Command),
            ("a b", PathKind::Subcommand),
            ("a b c", PathKind::GroupSubcommand),
        ] {
            assert_eq!(PathKind::of(path), Some(kind), "{path:?}");
            assert_eq!(kind.words(), path.split(' ').count());
        }
    }

    #[test]
    fn cmd_10_path_kind_rejects_malformed_paths() {
        for path in ["", "a b c d", "a  b", " a"] {
            assert_eq!(PathKind::of(path), None, "{path:?}");
        }
    }

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
