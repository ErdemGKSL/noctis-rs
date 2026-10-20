//! Every registration of an App: [`validate_merge`] (REG-01..04, META-03,
//! CTX-01 plus the tree rules per scope).

use std::collections::HashSet;

use super::{
    CommandDecl, CommandMeta, ContextMenuDecl, DeclKind, MenuKind, Scope, source_name,
    sources_note, validate_context_menu_name, validate_meta, validate_tree, with_optional_note,
};
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
/// `scope: global` / `scope: guild <id>` note), then META-03 per command and
/// then per context-menu command ([`validate_meta`](super::validate_meta) with
/// the declaration's scope; the path is `<path or name>.<field>`), then CTX-01
/// per context-menu name ([`validate_context_menu_name`](super::validate_context_menu_name);
/// the path is the name), each in input order with a ``declared by `src` ``
/// note before the scope note, then the counts against
/// `limits.chat_input_commands` (distinct first words of non-empty handler
/// paths, so a merged base counts once), `limits.user_commands` and
/// `limits.message_commands` (REG-01 for the global scope, REG-02 for a guild;
/// a ``registered by `a`, `b` `` note names the distinct sources of the counted
/// declarations when any is named), then REG-03: two context-menu commands of the same type with the same name
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
        let menus_in_scope: Vec<&ContextMenuDecl<'_>> =
            menus.iter().filter(|m| m.scope == scope).collect();

        check_declarations(&in_scope, &menus_in_scope, scope, &note, limits, &mut out);

        let count_rule = match scope {
            Scope::Global => RuleId::REG_01,
            Scope::Guild(_) => RuleId::REG_02,
        };
        let counted: Vec<&CommandDecl<'_>> = in_scope
            .iter()
            .filter(|c| matches!(c.kind, DeclKind::Handler { .. }) && !c.path.is_empty())
            .collect();
        let bases: HashSet<&str> = counted
            .iter()
            .filter_map(|c| c.path.split(' ').next())
            .collect();
        let menus_of = |kind| menus_in_scope.iter().filter(move |m| m.kind == kind);
        for (what, count, max, sources) in [
            (
                "chat-input commands",
                bases.len(),
                limits.chat_input_commands,
                counted.iter().map(|c| c.source).collect::<Vec<_>>(),
            ),
            (
                "user commands",
                menus_of(MenuKind::User).count(),
                limits.user_commands,
                menus_of(MenuKind::User).map(|m| m.source).collect(),
            ),
            (
                "message commands",
                menus_of(MenuKind::Message).count(),
                limits.message_commands,
                menus_of(MenuKind::Message).map(|m| m.source).collect(),
            ),
        ] {
            if count > max {
                out.push(
                    with_optional_note(
                        Violation::new(count_rule, what, Bound::AtMost(max), Actual::Count(count)),
                        sources_note("registered by", sources),
                    )
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

/// META-03 for commands, then for menus, then CTX-01 per menu name.
fn check_declarations(
    commands: &[CommandDecl<'_>],
    menus: &[&ContextMenuDecl<'_>],
    scope: Scope,
    note: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let metas = commands
        .iter()
        .map(|c| (c.source, c.path, &c.metadata))
        .chain(menus.iter().map(|m| (m.source, m.name, &m.metadata)));
    for (source, path, metadata) in metas {
        out.extend(meta_03(source, path, metadata, scope, limits));
    }
    for menu in menus {
        for v in validate_context_menu_name(menu.name, limits) {
            out.push(
                Violation {
                    path: menu.name.to_owned(),
                    ..v
                }
                .with_note(format!("declared by {}", source_name(menu.source)))
                .with_note(note),
            );
        }
    }
}

/// META-03 for one declaration: `path.field`, a ``declared by `src` `` note
/// before the scope note.
fn meta_03(
    source: &str,
    path: &str,
    metadata: &CommandMeta<'_>,
    scope: Scope,
    limits: &Limits,
) -> impl Iterator<Item = Violation> {
    let declared = format!("declared by {}", source_name(source));
    let path = path.to_owned();
    validate_meta(metadata, scope, limits)
        .into_iter()
        .map(move |v| {
            let mut notes = vec![declared.clone()];
            notes.extend(v.notes.iter().cloned());
            Violation {
                path: format!("{path}.{}", v.path),
                notes,
                ..v
            }
        })
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

    fn perm(p: u64) -> CommandMeta<'static> {
        CommandMeta::new().default_member_permissions(p)
    }

    #[test]
    fn meta_01_scopes_are_separate() {
        assert_eq!(
            check(
                &[
                    h("m1", "config").metadata(perm(8)),
                    h("m2", "config").metadata(perm(32)).scope(Scope::Guild(1)),
                ],
                &[]
            ),
            []
        );
    }

    #[test]
    fn meta_01_same_scope_conflicts() {
        let errs = check(
            &[
                h("m1", "config set").metadata(perm(8)),
                h("m2", "config get").metadata(perm(32)),
            ],
            &[],
        );
        assert_eq!(rules(&errs), [RuleId::META_01]);
        assert_eq!(
            errs[0].notes.last().map(String::as_str),
            Some("scope: global")
        );
        // The same path twice is a REG-04 duplicate as well.
        let errs = check(
            &[
                h("m1", "config").metadata(perm(8)),
                h("m2", "config").metadata(perm(32)),
            ],
            &[],
        );
        assert_eq!(rules(&errs), [RuleId::REG_04, RuleId::META_01]);
        assert_eq!(
            errs[1].notes.last().map(String::as_str),
            Some("scope: global")
        );
    }

    #[test]
    fn meta_03_guild_command_with_contexts() {
        use crate::command::InteractionContext;
        let meta = CommandMeta::new().contexts(&[InteractionContext::Guild]);
        let errs = check(
            &[h("m1", "ping").scope(Scope::Guild(42)).metadata(meta)],
            &[],
        );
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::META_03,
                "ping.contexts",
                Bound::Describe("only on global commands"),
                Actual::Text("[Guild]".into())
            )
            .with_note("declared by `m1`")
            .with_note("scope: guild 42")]
        );
        let errs = check(&[h("", "ping").scope(Scope::Guild(42)).metadata(meta)], &[]);
        assert_eq!(errs[0].notes[0], "declared by (unnamed)");
    }

    #[test]
    fn meta_03_guild_context_menu_with_integration_types() {
        use crate::command::IntegrationType;
        let menu = ContextMenuDecl::new("m1", MenuKind::User, "Show Profile")
            .scope(Scope::Guild(42))
            .metadata(CommandMeta::new().integration_types(&[IntegrationType::GuildInstall]));
        let errs = check(&[], &[menu]);
        assert_eq!(rules(&errs), [RuleId::META_03]);
        assert_eq!(errs[0].path, "Show Profile.integration_types");
        assert_eq!(errs[0].notes, ["declared by `m1`", "scope: guild 42"]);
    }

    #[test]
    fn ctx_01_context_menu_names() {
        let a33 = "a".repeat(33);
        let errs = check(&[], &[ContextMenuDecl::new("m1", MenuKind::User, &a33)]);
        assert_eq!(
            errs,
            [Violation::new(
                RuleId::CTX_01,
                a33.as_str(),
                Bound::Between { min: 1, max: 32 },
                Actual::Count(33)
            )
            .with_note("declared by `m1`")
            .with_note("scope: global")]
        );
        let errs = check(&[], &[ContextMenuDecl::new("m1", MenuKind::Message, "")]);
        assert_eq!(rules(&errs), [RuleId::CTX_01]);
        assert_eq!(errs[0].path, "");
        assert_eq!(errs[0].actual, Actual::Count(0));
        assert_eq!(
            check(
                &[],
                &[ContextMenuDecl::new("m1", MenuKind::User, "Show Profile")]
            ),
            []
        );
    }

    #[test]
    fn meta_03_ctx_01_order_per_scope() {
        use crate::command::InteractionContext;
        let guild = Scope::Guild(7);
        let ctx = CommandMeta::new().contexts(&[InteractionContext::Guild]);
        let a33 = "a".repeat(33);
        let limits = Limits {
            user_commands: 1,
            ..Limits::default()
        };
        let commands = [
            h("", "Ping").scope(guild),
            h("", "b").scope(guild).metadata(ctx),
            h("", "c").scope(guild).metadata(ctx),
        ];
        let menus = [
            ContextMenuDecl::new("", MenuKind::User, &a33).scope(guild),
            ContextMenuDecl::new("", MenuKind::User, "X")
                .scope(guild)
                .metadata(ctx),
            ContextMenuDecl::new("", MenuKind::User, "X").scope(guild),
        ];
        let errs = check_with(&commands, &menus, &limits);
        let got: Vec<(RuleId, &str)> = errs.iter().map(|v| (v.rule, v.path.as_str())).collect();
        assert_eq!(
            got,
            [
                (RuleId::CMD_03, "Ping"),
                (RuleId::META_03, "b.contexts"),
                (RuleId::META_03, "c.contexts"),
                (RuleId::META_03, "X.contexts"),
                (RuleId::CTX_01, a33.as_str()),
                (RuleId::REG_02, "user commands"),
                (RuleId::REG_03, "X"),
            ]
        );
    }

    #[test]
    fn reg_01_note_names_the_registering_sources() {
        let bases = names(101);
        let decls: Vec<_> = bases
            .iter()
            .enumerate()
            .map(|(i, p)| h(if i < 50 { "a" } else { "b" }, p))
            .collect();
        assert_eq!(
            check(&decls, &[]),
            [Violation::new(
                RuleId::REG_01,
                "chat-input commands",
                Bound::AtMost(100),
                Actual::Count(101)
            )
            .with_note("registered by `a`, `b`")
            .with_note("scope: global")]
        );
        let user: Vec<_> = bases[..16]
            .iter()
            .map(|n| ContextMenuDecl::new("a", MenuKind::User, n))
            .collect();
        let errs = check(&[], &user);
        assert_eq!(errs[0].notes, ["registered by `a`", "scope: global"]);
    }

    #[test]
    fn reg_02_note_before_the_scope_note() {
        let bases = names(101);
        let decls: Vec<_> = bases
            .iter()
            .map(|p| h("a", p).scope(Scope::Guild(1)))
            .collect();
        let errs = check(&decls, &[]);
        assert_eq!(rules(&errs), [RuleId::REG_02]);
        assert_eq!(errs[0].notes, ["registered by `a`", "scope: guild 1"]);
    }

    #[test]
    fn cmd_13_tree_notes_come_before_the_scope_note() {
        let names: Vec<String> = (0..26).map(|i| format!("x s{i}")).collect();
        let decls: Vec<_> = names.iter().map(|p| h("a", p)).collect();
        let errs = check(&decls, &[]);
        assert_eq!(rules(&errs), [RuleId::CMD_13]);
        assert_eq!(errs[0].notes, ["contributed by `a`", "scope: global"]);
    }
}
