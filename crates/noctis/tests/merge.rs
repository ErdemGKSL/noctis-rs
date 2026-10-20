//! App-build merge and conflict checks for Commands end to end: `#[command]`
//! and the context-menu macros, `Module::base`/`group`, `App::build` and the
//! merged command tree (Story 6.4, Matrix 1, 3, 11, 13, 15, 17, 26, 28, 30).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use noctis::__private::core::__testing::command_tree;
use noctis::prelude::*;
use noctis::rules::{RuleId, Violation};
use noctis::ui::ResolvedUser;
use noctis::{BaseMeta, BuildError, BuildViolation, Permissions, Runtime};

#[command("config set", description = "Set a value")]
async fn config_set(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config get prefix", description = "Show the prefix")]
async fn config_get_prefix(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config get", description = "Read a value")]
async fn config_get(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config set", description = "Set a value", guilds = [42])]
async fn guild_config_set(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config set", description = "Set a value", permissions = MANAGE_GUILD)]
async fn managed_config_set(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config get", description = "Read a value", permissions = ADMINISTRATOR)]
async fn admin_config_get(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config", description = "Configure")]
async fn config(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[command("config (set|get)", description = "Set a value")]
async fn config_any(_ctx: CommandCtx, _path: ConfigAnyPath) -> HandlerResult<()> {
    Ok(())
}

#[command("show profile", description = "Show a profile")]
async fn show_profile(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

#[user_command("Show profile")]
async fn show_user(_ctx: CommandCtx, _target: ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

#[user_command("Show profile")]
async fn show_user_again(_ctx: CommandCtx, _target: ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

#[message_command("Show profile")]
async fn show_message(_ctx: CommandCtx, _target: noctis::ui::ResolvedMessage) -> HandlerResult<()> {
    Ok(())
}

fn build(modules: impl IntoIterator<Item = Module>) -> Result<App, BuildError> {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    for module in modules {
        builder.module(module);
    }
    builder.build()
}

fn tree(modules: impl IntoIterator<Item = Module>) -> String {
    command_tree(&build(modules).unwrap())
}

/// The command violations of a failing build.
fn failing(modules: impl IntoIterator<Item = Module>) -> Vec<Violation> {
    let Err(err) = build(modules) else {
        panic!("build passed");
    };
    err.into_violations()
        .into_iter()
        .map(|v| match v {
            BuildViolation::Command(v) => v,
            other => panic!("not a command violation: {other:?}"),
        })
        .collect()
}

fn name(f: &str) -> String {
    format!("{}::{f}", module_path!())
}

/// Matrix 1: a base split over two Modules merges into one `/config`.
#[tokio::test]
async fn reg_04_merge_across_modules() {
    let tree = tree([
        Module::new("a").add(config_set()),
        Module::new("b").add(config_get_prefix()),
    ]);
    assert_eq!(
        tree,
        format!(
            "global\n  /config \"Set a value\"\n    set \"Set a value\" -> {} (a)\n    \
             get \"Show the prefix\"\n      prefix \"Show the prefix\" -> {} (b)",
            name("config_set"),
            name("config_get_prefix")
        )
    );
}

/// Matrix 3: an explicit base description from another Module.
#[tokio::test]
async fn cmd_04_explicit_base_description() {
    let tree = tree([
        Module::new("a").add(config_set()),
        Module::new("b").base("config", BaseMeta::new().description("Configure")),
    ]);
    assert!(
        tree.starts_with("global\n  /config \"Configure\"\n"),
        "{tree}"
    );
}

/// Matrix 11: guild and global scopes never merge.
#[tokio::test]
async fn reg_02_guild_scope_is_separate() {
    let tree = tree([
        Module::new("a").add(guild_config_set()),
        Module::new("b").add(config_get()),
    ]);
    assert_eq!(
        tree,
        format!(
            "guild 42\n  /config \"Set a value\"\n    set \"Set a value\" -> {} (a)\n\
             global\n  /config \"Read a value\"\n    get \"Read a value\" -> {} (b)",
            name("guild_config_set"),
            name("config_get")
        )
    );
}

/// Matrix 13: explicit base metadata; an unset handler inherits it.
#[tokio::test]
async fn meta_01_base_meta_resolution() {
    let tree = tree([
        Module::new("a").add(managed_config_set()),
        Module::new("b").base(
            "config",
            BaseMeta::new()
                .default_member_permissions(Permissions::MANAGE_GUILD)
                .nsfw(false),
        ),
        Module::new("c").add(config_get()),
    ]);
    assert!(
        tree.starts_with("global\n  /config \"Set a value\" permissions=32 nsfw=false\n"),
        "{tree}"
    );
}

/// Matrix 15: user, message and chat-input commands never clash.
#[tokio::test]
async fn reg_03_menu_types_and_chat_input() {
    let tree = tree([
        Module::new("a").add(show_user()),
        Module::new("b").add(show_message()),
        Module::new("c").add(show_profile()),
    ]);
    assert!(tree.contains("  user \"Show profile\" -> "), "{tree}");
    assert!(tree.contains("  message \"Show profile\" -> "), "{tree}");
    assert!(tree.contains("  /show \"Show a profile\"\n"), "{tree}");
}

/// Matrix 17: conflicting permissions across Modules.
#[tokio::test]
async fn meta_01_permissions_conflict() {
    let errs = failing([
        Module::new("a").add(managed_config_set()),
        Module::new("b").add(admin_config_get()),
    ]);
    assert_eq!(errs.len(), 1);
    assert_eq!(
        (errs[0].rule, errs[0].path.as_str()),
        (RuleId::META_01, "config")
    );
    assert_eq!(
        errs[0].notes,
        [
            "`config set` sets `32` (from `a`)",
            "`config get` sets `8` (from `b`)",
            "scope: global"
        ]
    );
    let message = build([
        Module::new("a").add(managed_config_set()),
        Module::new("b").add(admin_config_get()),
    ])
    .unwrap_err()
    .to_string();
    assert!(message.contains("[META-01] config:"), "{message}");
}

/// Matrix 26: an invokable base with subcommands from another Module.
#[tokio::test]
async fn cmd_12_invokable_base_across_modules() {
    let errs = failing([
        Module::new("a").add(config()),
        Module::new("b").add(config_set()),
    ]);
    assert_eq!(errs.len(), 1);
    assert_eq!(
        (errs[0].rule, errs[0].path.as_str()),
        (RuleId::CMD_12, "config")
    );
    assert_eq!(
        errs[0].notes,
        [
            "`config` makes `config` invokable (from `a`)",
            "`config set` gives `config` subcommands (from `b`)",
            "scope: global"
        ]
    );
}

/// Matrix 28: the same path in two Modules.
#[tokio::test]
async fn reg_04_duplicate_path() {
    let errs = failing([
        Module::new("a").add(config_set()),
        Module::new("b").add(config_any()),
    ]);
    assert_eq!(errs.len(), 1);
    assert_eq!(
        (errs[0].rule, errs[0].path.as_str()),
        (RuleId::REG_04, "config set")
    );
    assert_eq!(
        errs[0].notes,
        ["registered by `a`", "registered by `b`", "scope: global"]
    );
}

/// Matrix 30: the same user command name in two Modules.
#[tokio::test]
async fn reg_03_same_user_command() {
    let errs = failing([
        Module::new("a").add(show_user()),
        Module::new("b").add(show_user_again()),
    ]);
    assert_eq!(errs.len(), 1);
    assert_eq!(
        (errs[0].rule, errs[0].path.as_str()),
        (RuleId::REG_03, "Show profile")
    );
    assert_eq!(
        errs[0].notes,
        ["registered by `a`", "registered by `b`", "scope: global"]
    );
}
