//! Every registration of an App: [`validate_merge`] (REG-01..04 plus the tree
//! rules per scope).

use std::collections::HashSet;

use super::{CommandDecl, ContextMenuDecl, DeclKind, MenuKind, Scope, source_name, validate_tree};
use crate::{Actual, Bound, Limits, RuleId, Violation};

fn scope_note(scope: Scope) -> String {
    match scope {
        Scope::Global => "scope: global".to_owned(),
        Scope::Guild(id) => format!("scope: guild {id}"),
    }
}

/// Checks all chat-input and context-menu declarations of an App after merging.
///
/// Scopes are checked in first appearance (commands, then menus). Per scope:
/// [`validate_tree`] on that scope's commands (every violation gains a
/// `scope: global` / `scope: guild <id>` note), then the counts against
/// `limits.chat_input_commands` (distinct first words of non-empty handler
/// paths, so a merged base counts once), `limits.user_commands` and
/// `limits.message_commands` (REG-01 for the global scope, REG-02 for a guild),
/// then REG-03: two context-menu commands of the same type with the same name
/// (byte equality), in first-occurrence order. Same-name chat-input handlers
/// are merged, so their clashes are REG-04 from the tree check, never REG-03.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::command::{ContextMenuDecl, MenuKind, validate_merge};
///
/// let menus = [
///     ContextMenuDecl::new("m1", MenuKind::User, "Show Profile"),
///     ContextMenuDecl::new("m2", MenuKind::User, "Show Profile"),
/// ];
/// let errs = validate_merge(&[], &menus, &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::REG_03);
/// ```
pub fn validate_merge(
    commands: &[CommandDecl<'_>],
    menus: &[ContextMenuDecl<'_>],
    limits: &Limits,
) -> Result<(), Vec<Violation>> {
    let mut scopes: Vec<Scope> = Vec::new();
    for scope in commands
        .iter()
        .map(|c| c.scope)
        .chain(menus.iter().map(|m| m.scope))
    {
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    let mut out = Vec::new();
    for scope in scopes {
        let note = scope_note(scope);
        let in_scope: Vec<CommandDecl<'_>> = commands
            .iter()
            .filter(|c| c.scope == scope)
            .copied()
            .collect();
        if let Err(errs) = validate_tree(&in_scope, limits) {
            out.extend(errs.into_iter().map(|v| v.with_note(note.clone())));
        }

        let count_rule = match scope {
            Scope::Global => RuleId::REG_01,
            Scope::Guild(_) => RuleId::REG_02,
        };
        let bases: HashSet<&str> = in_scope
            .iter()
            .filter(|c| matches!(c.kind, DeclKind::Handler { .. }) && !c.path.is_empty())
            .filter_map(|c| c.path.split(' ').next())
            .collect();
        let menus_in_scope: Vec<&ContextMenuDecl<'_>> =
            menus.iter().filter(|m| m.scope == scope).collect();
        let count_of = |kind| menus_in_scope.iter().filter(|m| m.kind == kind).count();
        for (what, count, max) in [
            (
                "chat-input commands",
                bases.len(),
                limits.chat_input_commands,
            ),
            (
                "user commands",
                count_of(MenuKind::User),
                limits.user_commands,
            ),
            (
                "message commands",
                count_of(MenuKind::Message),
                limits.message_commands,
            ),
        ] {
            if count > max {
                out.push(
                    Violation::new(count_rule, what, Bound::AtMost(max), Actual::Count(count))
                        .with_note(note.clone()),
                );
            }
        }

        let mut groups: Vec<(MenuKind, &str, Vec<&str>)> = Vec::new();
        for menu in &menus_in_scope {
            match groups
                .iter_mut()
                .find(|(kind, name, _)| *kind == menu.kind && *name == menu.name)
            {
                Some((_, _, sources)) => sources.push(menu.source),
                None => groups.push((menu.kind, menu.name, vec![menu.source])),
            }
        }
        for (_, name, sources) in groups.into_iter().filter(|(_, _, s)| s.len() > 1) {
            let mut v = Violation::new(
                RuleId::REG_03,
                name,
                Bound::Describe("one context-menu command per name, type and scope"),
                Actual::Text(name.to_owned()),
            );
            for source in sources {
                v = v.with_note(format!("registered by {}", source_name(source)));
            }
            out.push(v.with_note(note.clone()));
        }
    }

    if out.is_empty() { Ok(()) } else { Err(out) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::LocalizedText;

    const D: LocalizedText<'static> = LocalizedText::new("d");

    fn h<'a>(source: &'a str, path: &'a str) -> CommandDecl<'a> {
        CommandDecl::handler(source, path).description(D)
    }

    fn check(commands: &[CommandDecl<'_>], menus: &[ContextMenuDecl<'_>]) -> Vec<Violation> {
        check_with(commands, menus, &Limits::default())
    }

    fn check_with(
        commands: &[CommandDecl<'_>],
        menus: &[ContextMenuDecl<'_>],
        limits: &Limits,
    ) -> Vec<Violation> {
        match validate_merge(commands, menus, limits) {
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

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("c{i}")).collect()
    }

    fn menus(kind: MenuKind, names: &[String]) -> Vec<ContextMenuDecl<'_>> {
        names
            .iter()
            .map(|n| ContextMenuDecl::new("", kind, n))
            .collect()
    }

    #[test]
    fn reg_01_empty_is_ok() {
        assert_eq!(validate_merge(&[], &[], &Limits::default()), Ok(()));
    }

    #[test]
    fn reg_01_merged_base_counts_once() {
        let limits = Limits {
            chat_input_commands: 1,
            ..Limits::default()
        };
        assert_eq!(
            check_with(&[h("m1", "a b"), h("m2", "a c")], &[], &limits),
            []
        );
    }

    #[test]
    fn reg_01_chat_input_count() {
        let bases = names(101);
        let decls: Vec<_> = bases.iter().map(|p| h("", p)).collect();
        assert_eq!(check(&decls[..100], &[]), []);
        assert_eq!(
            check(&decls, &[]),
            [Violation::new(
                RuleId::REG_01,
                "chat-input commands",
                Bound::AtMost(100),
                Actual::Count(101)
            )
            .with_note("scope: global")]
        );
    }

    #[test]
    fn reg_02_guild_count() {
        let bases = names(101);
        let decls: Vec<_> = bases
            .iter()
            .map(|p| h("", p).scope(Scope::Guild(42)))
            .collect();
        assert_eq!(
            check(&decls, &[]),
            [Violation::new(
                RuleId::REG_02,
                "chat-input commands",
                Bound::AtMost(100),
                Actual::Count(101)
            )
            .with_note("scope: guild 42")]
        );
    }

    #[test]
    fn reg_02_scopes_are_separate() {
        let bases = names(100);
        let mut decls: Vec<_> = bases.iter().map(|p| h("", p)).collect();
        decls.extend(bases.iter().map(|p| h("", p).scope(Scope::Guild(42))));
        assert_eq!(check(&decls, &[]), []);
        assert_eq!(
            check(&[h("", "ping"), h("", "ping").scope(Scope::Guild(42))], &[]),
            []
        );
    }

    #[test]
    fn reg_01_user_and_message_counts() {
        let n15 = names(15);
        let n16 = names(16);
        assert_eq!(check(&[], &menus(MenuKind::User, &n15)), []);
        let errs = check(&[], &menus(MenuKind::User, &n16));
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::REG_01,
                "user commands",
                Bound::AtMost(15),
                Actual::Count(16)
            )
            .with_note("scope: global")]
        );
        let errs = check(&[], &menus(MenuKind::Message, &n16));
        assert_eq!(rules(&errs), [RuleId::REG_01]);
        assert_eq!(errs[0].path, "message commands");
        let limits = Limits {
            chat_input_commands: 1,
            ..Limits::default()
        };
        let errs = check_with(&[h("", "a"), h("", "b")], &[], &limits);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::REG_01,
                "chat-input commands",
                Bound::AtMost(1),
                Actual::Count(2)
            )
            .with_note("scope: global")]
        );
    }

    #[test]
    fn reg_01_meta_and_empty_paths_are_not_counted() {
        let limits = Limits {
            chat_input_commands: 1,
            ..Limits::default()
        };
        let errs = check_with(
            &[h("", "a"), CommandDecl::meta("", "b"), h("", "")],
            &[],
            &limits,
        );
        assert!(!errs.iter().any(|v| v.rule == RuleId::REG_01), "{errs:?}");
    }

    #[test]
    fn reg_03_same_context_menu_name() {
        let menus = [
            ContextMenuDecl::new("m1", MenuKind::User, "Show Profile"),
            ContextMenuDecl::new("m2", MenuKind::User, "Show Profile"),
        ];
        assert_eq!(
            check(&[], &menus),
            [Violation::new(
                RuleId::REG_03,
                "Show Profile",
                Bound::Describe("one context-menu command per name, type and scope"),
                Actual::Text("Show Profile".into())
            )
            .with_note("registered by `m1`")
            .with_note("registered by `m2`")
            .with_note("scope: global")]
        );
    }

    #[test]
    fn reg_03_allowed_name_sharing() {
        let name = "Show Profile";
        assert_eq!(
            check(
                &[],
                &[
                    ContextMenuDecl::new("", MenuKind::User, name),
                    ContextMenuDecl::new("", MenuKind::Message, name),
                ]
            ),
            []
        );
        assert_eq!(
            check(
                &[],
                &[
                    ContextMenuDecl::new("", MenuKind::User, name),
                    ContextMenuDecl::new("", MenuKind::User, name).scope(Scope::Guild(1)),
                ]
            ),
            []
        );
        assert_eq!(
            check(
                &[],
                &[
                    ContextMenuDecl::new("", MenuKind::User, "show profile"),
                    ContextMenuDecl::new("", MenuKind::User, name),
                ]
            ),
            []
        );
    }

    #[test]
    fn cmd_12_tree_rules_run_per_scope() {
        let errs = check(&[h("m1", "config"), h("m2", "config set")], &[]);
        assert_eq!(rules(&errs), [RuleId::CMD_12]);
        assert_eq!(
            errs[0].notes.last().map(String::as_str),
            Some("scope: global")
        );
        assert_eq!(errs[0].notes.len(), 3);
        assert_eq!(
            check(
                &[
                    h("m1", "config"),
                    h("m2", "config set").scope(Scope::Guild(42))
                ],
                &[]
            ),
            []
        );
    }

    #[test]
    fn reg_04_same_chat_input_name_is_a_duplicate_not_reg_03() {
        let errs = check(&[h("m1", "ping"), h("m2", "ping")], &[]);
        assert_eq!(rules(&errs), [RuleId::REG_04]);
    }

    #[test]
    fn reg_01_reg_03_order_per_scope() {
        let bases = names(2);
        let limits = Limits {
            chat_input_commands: 1,
            ..Limits::default()
        };
        let mut decls: Vec<_> = bases
            .iter()
            .map(|p| h("", p).scope(Scope::Guild(7)))
            .collect();
        decls.push(h("", "Ping"));
        let menus = [
            ContextMenuDecl::new("", MenuKind::User, "X"),
            ContextMenuDecl::new("", MenuKind::User, "X"),
        ];
        let errs = check_with(&decls, &menus, &limits);
        assert_eq!(
            rules(&errs),
            [RuleId::REG_02, RuleId::CMD_03, RuleId::REG_03]
        );
        assert_eq!(errs[0].notes, ["scope: guild 7"]);
    }
}
