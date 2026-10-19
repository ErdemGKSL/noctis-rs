//! The Story 1.5 acceptance criteria through the public API (OPT-01, OPT-05,
//! OPT-06, OPT-13, CTX-01, META-01, META-03).

use noctis_rules::command::{
    CommandDecl, CommandMeta, ContextMenuDecl, InteractionContext, LocalizedText, MenuKind,
    NumericValue, OptionDecl, OptionType, Scope, validate_context_menu_name, validate_merge,
    validate_options,
};
use noctis_rules::{Limits, RuleId};

const D: LocalizedText<'static> = LocalizedText::new("d");

#[test]
fn opt_01_opt_05_opt_06_one_list_reports_every_rule() {
    let options = [
        OptionDecl::new("reason", OptionType::String, "Why")
            .min_length(10)
            .max_length(5),
        OptionDecl::new("days", OptionType::Integer, "How long")
            .required(true)
            .min_value(NumericValue::Int(30))
            .max_value(NumericValue::Int(1)),
        OptionDecl::new("evidence", OptionType::Attachment, "Proof").file_types(&["pdf"]),
    ];
    let errs = validate_options(&options, &Limits::default()).unwrap_err();
    let got: Vec<(RuleId, &str)> = errs.iter().map(|v| (v.rule, v.path.as_str())).collect();
    assert_eq!(
        got,
        [
            (RuleId::OPT_06, "reason"),
            (RuleId::OPT_05, "days"),
            (RuleId::OPT_13, "evidence.file_types[0]"),
            (RuleId::OPT_01, "days"),
        ]
    );
    assert_eq!(
        errs[3].to_string(),
        "[OPT-01] days: limit required options before optional ones, actual `days`\n  \
         = note: after optional `reason`"
    );
}

#[test]
fn ctx_01_context_menu_name_length() {
    let limits = Limits::default();
    assert!(validate_context_menu_name("Show Profile", &limits).is_empty());
    let errs = validate_context_menu_name(&"a".repeat(33), &limits);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].rule, RuleId::CTX_01);
    assert_eq!(
        errs[0].to_string(),
        "[CTX-01] limit between 1 and 32, actual 33"
    );
}

#[test]
fn meta_01_meta_03_merge_reports_both() {
    let contexts = [InteractionContext::Guild];
    let commands = [
        CommandDecl::handler("m1", "config set")
            .description(D)
            .metadata(CommandMeta::new().default_member_permissions(8)),
        CommandDecl::handler("m2", "config get")
            .description(D)
            .metadata(CommandMeta::new().default_member_permissions(32)),
        CommandDecl::handler("m3", "ping")
            .description(D)
            .scope(Scope::Guild(42))
            .metadata(CommandMeta::new().contexts(&contexts)),
    ];
    let menus = [ContextMenuDecl::new("m1", MenuKind::User, "Show Profile")];
    let errs = validate_merge(&commands, &menus, &Limits::default()).unwrap_err();
    let rules: Vec<_> = errs.iter().map(|v| v.rule).collect();
    assert_eq!(rules, [RuleId::META_01, RuleId::META_03]);
    assert_eq!(errs[1].path, "ping.contexts");
}
