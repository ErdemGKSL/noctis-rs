//! The Story 1.4 acceptance criteria through the public API (CMD-11, CMD-15,
//! REG-01..04).

use noctis_rules::command::{
    CommandDecl, ContextMenuDecl, LocalizedText, MenuKind, expand, validate_merge, validate_tree,
};
use noctis_rules::{Limits, RuleId};

const D: LocalizedText<'static> = LocalizedText::new("d");

#[test]
fn cmd_15_expand_order() {
    let paths = expand("config (set|get) (prefix|welcome)", &Limits::default()).unwrap();
    assert_eq!(
        paths,
        [
            "config set prefix",
            "config set welcome",
            "config get prefix",
            "config get welcome"
        ]
    );
}

#[test]
fn cmd_11_display_names_both_contributing_paths() {
    let limits = Limits::default();
    let paths = expand("config (set|get) (prefix|welcome)?", &limits).unwrap();
    let decls: Vec<_> = paths
        .iter()
        .map(|p| CommandDecl::handler("config", p).description(D))
        .collect();
    let errs = validate_tree(&decls, &limits).unwrap_err();
    assert_eq!(errs.len(), 2);
    assert_eq!(
        errs[0].to_string(),
        "[CMD-11] config set: limit a subcommand or a subcommand group, not both, actual `set`\n  \
         = note: `config set` makes `set` a subcommand (from `config`)\n  \
         = note: `config set prefix` makes `set` a subcommand group (from `config`)"
    );
}

#[test]
fn reg_04_merge_reports_several_rules_at_once() {
    let commands = [
        CommandDecl::handler("m1", "config").description(D),
        CommandDecl::handler("m2", "config set").description(D),
        CommandDecl::handler("m1", "ping").description(D),
        CommandDecl::handler("m2", "ping").description(D),
    ];
    let menus = [
        ContextMenuDecl::new("m1", MenuKind::User, "Show Profile"),
        ContextMenuDecl::new("m2", MenuKind::User, "Show Profile"),
    ];
    let errs = validate_merge(&commands, &menus, &Limits::default()).unwrap_err();
    let rules: Vec<_> = errs.iter().map(|v| v.rule).collect();
    assert_eq!(rules, [RuleId::REG_04, RuleId::CMD_12, RuleId::REG_03]);
}
