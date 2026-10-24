//! Story 6.4 Matrix 1-16: the merged command Registry, its rendering and the
//! description fallback warnings.

use noctis_rules::command::{IntegrationType, InteractionContext};
use twilight_model::gateway::Intents;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use super::*;
use crate::events::MessageCreate;
use crate::metadata::BaseMeta;
use crate::registry::Registry;
use crate::test_support::{Capture, command, command_with, message_menu, noop, user_menu};

/// Folds `modules` and returns the rendered tree, the warnings and the
/// violations.
fn fold(modules: &[Module]) -> (String, Vec<String>, Vec<BuildViolation>) {
    fold_with(modules, None)
}

/// [`fold`] with the merged `strings`.
fn fold_with(
    modules: &[Module],
    strings: Option<&AppStrings>,
) -> (String, Vec<String>, Vec<BuildViolation>) {
    let capture = Capture::default();
    let (registry, violations) = {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may register the `warn!` callsite with no subscriber,
        // caching `Interest::never`; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        Registry::fold(
            modules,
            Intents::empty(),
            strings,
            &crate::BootFlags::all(),
            &crate::namespace::fallback_app_id(),
        )
    };
    let warnings = capture.lines_with(&["noctis::registry", "WARN"]);
    (registry.commands().render(), warnings, violations)
}

/// Folds `modules`, which must pass, and returns the tree and the warnings.
fn ok(modules: &[Module]) -> (String, Vec<String>) {
    let (tree, warnings, violations) = fold(modules);
    assert_eq!(violations, [], "{tree}");
    (tree, warnings)
}

fn warning(node: &str, scope: &str, path: &str, module: &str) -> String {
    format!(
        "no description declared for `{node}` ({scope}): using the description of `{path}` from \
         Module `{module}`; declare one with `Module::base` or `Module::group`"
    )
}

fn assert_warnings(got: &[String], expected: &[String]) {
    assert_eq!(got.len(), expected.len(), "{got:#?}");
    for (line, text) in got.iter().zip(expected) {
        assert!(line.contains(text.as_str()), "{line}\nexpected: {text}");
    }
}

/// Matrix 1: a base split over two Modules merges.
#[test]
fn reg_04_merge_across_modules() {
    let a = Module::new("a").__add(command("a::set", &["config set"], "Set a value"));
    let b = Module::new("b").__add(command(
        "b::prefix",
        &["config get prefix"],
        "Show the prefix",
    ));
    let (tree, warnings) = ok(&[a, b]);
    assert_eq!(
        tree,
        "global\n  \
         /config \"Set a value\"\n    \
         set \"Set a value\" -> a::set (a)\n    \
         get \"Show the prefix\"\n      \
         prefix \"Show the prefix\" -> b::prefix (b)"
    );
    assert_warnings(
        &warnings,
        &[
            warning("config", "scope: global", "config set", "a"),
            warning("config get", "scope: global", "config get prefix", "b"),
        ],
    );
}

/// Matrix 2: one pattern in one Module.
#[test]
fn reg_04_same_module_pattern() {
    let a = Module::new("a").__add(command("a::config", &["config set", "config get"], "d"));
    let (tree, warnings) = ok(&[a]);
    assert_eq!(
        tree,
        "global\n  /config \"d\"\n    set \"d\" -> a::config (a)\n    get \"d\" -> a::config (a)"
    );
    assert_warnings(
        &warnings,
        &[warning("config", "scope: global", "config get", "a")],
    );
}

/// Matrix 3: an explicit base description from another Module.
#[test]
fn cmd_04_explicit_base_description() {
    let a = Module::new("a").__add(command("a::set", &["config set"], "d"));
    let b = Module::new("b").base("config", BaseMeta::new().description("Configure"));
    let (tree, warnings) = ok(&[a, b]);
    assert!(
        tree.starts_with("global\n  /config \"Configure\"\n"),
        "{tree}"
    );
    assert_eq!(warnings, Vec::<String>::new());
}

/// Matrix 4: an explicit group description.
#[test]
fn cmd_04_explicit_group_description() {
    let a = Module::new("a")
        .__add(command("a::prefix", &["config set prefix"], "d"))
        .group("config set", "Set values");
    let (tree, warnings) = ok(&[a]);
    assert!(tree.contains("    set \"Set values\"\n"), "{tree}");
    assert_warnings(
        &warnings,
        &[warning("config", "scope: global", "config set prefix", "a")],
    );
}

/// Matrix 5: the same explicit value twice is no conflict.
#[test]
fn reg_04_same_explicit_value_twice() {
    let c = BaseMeta::new().description("C");
    let a = Module::new("a")
        .__add(command("a::set", &["config set"], "d"))
        .base("config", c);
    let b = Module::new("b").base("config", c);
    let (tree, warnings) = ok(&[a, b]);
    assert!(tree.contains("/config \"C\""), "{tree}");
    assert_eq!(warnings, Vec::<String>::new());
}

/// Matrix 6: no commands, an empty tree.
#[test]
fn reg_01_no_commands() {
    let a = Module::new("a").__add(noop::<MessageCreate>("h"));
    assert_eq!(ok(&[a]), (String::new(), Vec::new()));
    assert_eq!(ok(&[]), (String::new(), Vec::new()));
}

/// Matrix 7: plain 1-word commands keep their own descriptions.
#[test]
fn cmd_04_plain_bases() {
    let a = Module::new("a").__add(command("a::ping", &["ping"], "Ping"));
    let b = Module::new("b").__add(command("b::pong", &["pong"], "Pong"));
    let (tree, warnings) = ok(&[a, b]);
    assert_eq!(
        tree,
        "global\n  /ping \"Ping\" -> a::ping (a)\n  /pong \"Pong\" -> b::pong (b)"
    );
    assert_eq!(warnings, Vec::<String>::new());
}

/// Matrix 8: the smallest Module id wins, not the mount order.
#[test]
fn cmd_04_fallback_uses_the_smallest_module_id() {
    let zeta = Module::new("zeta").__add(command("zeta::a", &["cfg a"], "From zeta"));
    let alpha = Module::new("alpha").__add(command("alpha::z", &["cfg z"], "From alpha"));
    let (tree, warnings) = ok(&[zeta, alpha]);
    assert!(tree.starts_with("global\n  /cfg \"From alpha\""), "{tree}");
    assert_warnings(
        &warnings,
        &[warning("cfg", "scope: global", "cfg z", "alpha")],
    );
}

/// Matrix 9: the fallback inside a group.
#[test]
fn cmd_04_fallback_in_a_group() {
    let b = Module::new("b").__add(command("b::s2", &["x g s2"], "S2"));
    let a = Module::new("a")
        .__add(command("a::s1", &["x g s1"], "S1"))
        .__add(command("a::h", &["x h"], "H"));
    let (tree, warnings) = ok(&[b, a]);
    assert!(
        tree.starts_with("global\n  /x \"S1\"\n    g \"S1\"\n"),
        "{tree}"
    );
    assert_warnings(
        &warnings,
        &[
            warning("x", "scope: global", "x g s1", "a"),
            warning("x g", "scope: global", "x g s1", "a"),
        ],
    );
}

/// Matrix 10: Unicode words merge and render unchanged.
#[test]
fn cmd_03_unicode_words() {
    let a = Module::new("a")
        .__add(command("a::dil", &["ayarlar dil"], "Dil"))
        .__add(command("a::gengo", &["設定 言語"], "言語"));
    let (tree, _) = ok(&[a]);
    assert_eq!(
        tree,
        "global\n  /ayarlar \"Dil\"\n    dil \"Dil\" -> a::dil (a)\n  \
         /設定 \"言語\"\n    言語 \"言語\" -> a::gengo (a)"
    );
}

const GUILD_42: &[Id<GuildMarker>] = &[Id::new(42)];

/// Matrix 11: scopes never merge.
#[test]
fn reg_02_guild_scope_is_separate() {
    let a = Module::new("a").__add(command_with(
        "a::set",
        &["config set"],
        CommandMetadata::new().guilds(GUILD_42),
    ));
    let b = Module::new("b").__add(command("b::get", &["config get"], "d"));
    let (tree, warnings) = ok(&[a, b]);
    assert_eq!(
        tree,
        "guild 42\n  /config \"d\"\n    set \"d\" -> a::set (a)\n\
         global\n  /config \"d\"\n    get \"d\" -> b::get (b)"
    );
    assert_warnings(
        &warnings,
        &[
            warning("config", "scope: guild 42", "config set", "a"),
            warning("config", "scope: global", "config get", "b"),
        ],
    );
}

/// Matrix 12: one command in two guilds.
#[test]
fn reg_02_two_guilds() {
    const GUILDS: &[Id<GuildMarker>] = &[Id::new(1), Id::new(2)];
    let a = Module::new("a").__add(command_with(
        "a::ping",
        &["ping"],
        CommandMetadata::new().guilds(GUILDS),
    ));
    let (registry, violations) = Registry::fold(
        &[a],
        Intents::empty(),
        None,
        &crate::BootFlags::all(),
        &crate::namespace::fallback_app_id(),
    );
    assert_eq!(violations, []);
    assert_eq!(
        registry.commands().render(),
        "guild 1\n  /ping \"d\" -> a::ping (a)\nguild 2\n  /ping \"d\" -> a::ping (a)"
    );
    for guild in [1, 2] {
        let scope = registry.commands().scope(Scope::Guild(guild)).unwrap();
        assert_eq!(scope.command("ping").unwrap().handler.name(), "a::ping");
    }
    assert!(registry.commands().scope(Scope::Global).is_none());
}

/// Matrix 13: base metadata from an explicit declaration; unset inherits.
#[test]
fn meta_01_base_meta_resolution() {
    let a = Module::new("a").__add(command_with(
        "a::set",
        &["config set"],
        CommandMetadata::new().default_member_permissions(Permissions::MANAGE_GUILD),
    ));
    let b = Module::new("b").base(
        "config",
        BaseMeta::new()
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .nsfw(false),
    );
    let c = Module::new("c").__add(command("c::get", &["config get"], "d"));
    let (tree, _) = ok(&[a, b, c]);
    assert!(
        tree.starts_with("global\n  /config \"d\" permissions=32 nsfw=false\n"),
        "{tree}"
    );
}

/// Matrix 14: agreeing handlers compare as sets and keep the first order.
#[test]
fn meta_02_agreeing_contexts() {
    use InteractionContext::{BotDm, Guild};
    let a = Module::new("a").__add(command_with(
        "a::set",
        &["config set"],
        CommandMetadata::new()
            .contexts(&[Guild, BotDm])
            .integration_types(&[IntegrationType::GuildInstall]),
    ));
    let b = Module::new("b").__add(command_with(
        "b::get",
        &["config get"],
        CommandMetadata::new()
            .contexts(&[BotDm, Guild])
            .integration_types(&[IntegrationType::GuildInstall]),
    ));
    let (tree, _) = ok(&[a, b]);
    assert!(
        tree.starts_with(
            "global\n  /config \"d\" contexts=[Guild, BotDm] integration_types=[GuildInstall]\n"
        ),
        "{tree}"
    );
}

/// Matrix 15: menu types never clash, chat-input is separate.
#[test]
fn reg_03_menu_types_and_chat_input() {
    let a = Module::new("a").__add(user_menu("a::show", "Show profile", CommandMetadata::new()));
    let b = Module::new("b").__add(message_menu(
        "b::show",
        "Show profile",
        CommandMetadata::new().nsfw(true),
    ));
    let c = Module::new("c").__add(command("c::show", &["show profile"], "Show"));
    let (registry, violations) = Registry::fold(
        &[a, b, c],
        Intents::empty(),
        None,
        &crate::BootFlags::all(),
        &crate::namespace::fallback_app_id(),
    );
    assert_eq!(violations, []);
    assert_eq!(
        registry.commands().render(),
        "global\n  /show \"Show\"\n    profile \"Show\" -> c::show (c)\n  \
         user \"Show profile\" -> a::show (a)\n  \
         message \"Show profile\" nsfw=true -> b::show (b)"
    );
    let global = registry.commands().scope(Scope::Global).unwrap();
    let user = global.menu(MenuKind::User, "Show profile").unwrap();
    assert_eq!((user.module, user.handler.name()), ("a", "a::show"));
    let message = global.menu(MenuKind::Message, "Show profile").unwrap();
    assert_eq!(message.module, "b");
    assert!(global.menu(MenuKind::User, "Show Profile").is_none());
    assert_eq!(global.command("show profile").unwrap().module, "c");
    assert!(global.command("show").is_none());
}

/// Matrix 16: a guild menu.
#[test]
fn reg_02_menu_in_guild() {
    const GUILD_7: &[Id<GuildMarker>] = &[Id::new(7)];
    let a = Module::new("a").__add(user_menu(
        "a::show",
        "Show",
        CommandMetadata::new().guilds(GUILD_7),
    ));
    let (tree, _) = ok(&[a]);
    assert_eq!(tree, "guild 7\n  user \"Show\" -> a::show (a)");
}

/// The entries carry the Module's ancestors.
#[test]
fn reg_04_entries_carry_ancestors() {
    let child = Module::new("p/c")
        .__add(command("c::ping", &["ping"], "d"))
        .__add(user_menu("c::show", "Show", CommandMetadata::new()));
    let root = Module::new("p").module(child);
    let (registry, violations) = Registry::fold(
        &[root],
        Intents::empty(),
        None,
        &crate::BootFlags::all(),
        &crate::namespace::fallback_app_id(),
    );
    assert_eq!(violations, []);
    let global = registry.commands().scope(Scope::Global).unwrap();
    assert_eq!(&*global.command("ping").unwrap().ancestors, ["p"]);
    assert_eq!(
        &*global.menu(MenuKind::User, "Show").unwrap().ancestors,
        ["p"]
    );
}

/// No fallback warning when the checks fail; the Registry is empty.
#[test]
fn cmd_12_failure_logs_no_warning_and_registers_nothing() {
    let a = Module::new("a")
        .__add(command("a::config", &["config"], "d"))
        .__add(command("a::set", &["config set"], "d"))
        .__add(command("a::x", &["x y"], "d"));
    let (tree, warnings, violations) = fold(&[a]);
    assert_eq!(tree, "");
    assert_eq!(warnings, Vec::<String>::new());
    assert!(
        matches!(&violations[..], [BuildViolation::Command(v)] if v.rule == noctis_rules::RuleId::CMD_12)
    );
}

/// The word-prefix match stops at a word boundary: `cfgx b` is not below `cfg`.
#[test]
fn cmd_04_fallback_respects_word_boundaries() {
    let a = Module::new("a").__add(command("a::b", &["cfgx b"], "X").__with_metadata(
        CommandMetadata::new().default_member_permissions(Permissions::ADMINISTRATOR),
    ));
    let b = Module::new("b").__add(command("b::a", &["cfg a"], "Y"));
    let (tree, warnings) = ok(&[a, b]);
    let cfg = tree
        .lines()
        .find(|line| line.starts_with("  /cfg "))
        .unwrap_or_default();
    assert_eq!(cfg, "  /cfg \"Y\"", "{tree}");
    assert!(
        warnings
            .iter()
            .any(|line| line.contains(&warning("cfg", "scope: global", "cfg a", "b"))),
        "{warnings:#?}"
    );
}

/// A `Module::base` declaration applies in every scope its base exists in.
#[test]
fn meta_01_base_meta_in_two_scopes() {
    let a = Module::new("a")
        .__add(command("a::set", &["config set"], "d"))
        .__add(command_with(
            "a::get",
            &["config get"],
            CommandMetadata::new().guilds(GUILD_42),
        ))
        .base(
            "config",
            BaseMeta::new()
                .description("Configure")
                .default_member_permissions(Permissions::MANAGE_GUILD),
        );
    let (tree, warnings) = ok(&[a]);
    assert_eq!(
        tree,
        "global\n  /config \"Configure\" permissions=32\n    set \"d\" -> a::set (a)\n\
         guild 42\n  /config \"Configure\" permissions=32\n    get \"d\" -> a::get (a)"
    );
    assert!(
        !warnings.iter().any(|line| line.contains("`config`")),
        "{warnings:#?}"
    );
}

/// Story 11.4 Matrix 20-22: an undescribed base or group takes
/// `cmd.<path>.description` from its catalog's default-locale file without
/// a warning; a declared description wins; with neither, the handler
/// fallback warns as before.
#[test]
fn cmd_04_catalog_description_before_the_fallback() {
    static BOT: noctis_i18n::Catalog = noctis_i18n::Catalog::__new(
        "bot/locales",
        noctis_rules::Locale::EnUs,
        &[
            (
                noctis_rules::Locale::EnUs,
                &[
                    ("cmd.config.get.description", "Show settings"),
                    ("cmd.setup.description", "Set the bot up"),
                ],
            ),
            (
                noctis_rules::Locale::Tr,
                &[("cmd.config.description", "Ayarlar")],
            ),
        ],
        &[],
    );
    let strings = AppStrings::new(&[BOT]).unwrap();
    let modules = [Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .__add(command("m::prefix", &["config get prefix"], "Prefix"))];
    let (tree, warnings, violations) = fold_with(&modules, Some(&strings));
    assert_eq!(violations, []);
    assert_eq!(
        tree,
        "global\n  \
         /setup \"Set the bot up\"\n    \
         start \"Start\" -> m::start (m)\n  \
         /config \"Prefix\"\n    \
         get \"Show settings\"\n      \
         prefix \"Prefix\" -> m::prefix (m)"
    );
    // Only `config` (whose key is in no default-locale file) falls back.
    assert_warnings(
        &warnings,
        &[warning("config", "scope: global", "config get prefix", "m")],
    );
    let declared = [Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .base("setup", BaseMeta::new().description("Set the bot up"))];
    let (tree, warnings, violations) = fold_with(&declared, Some(&strings));
    assert_eq!(violations, []);
    assert!(tree.contains("/setup \"Set the bot up\""), "{tree}");
    assert_eq!(warnings, Vec::<String>::new());
    // A differing declaration is CMD-04 (review pass 1).
    let differing = [Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .base("setup", BaseMeta::new().description("X"))];
    let (_, _, violations) = fold_with(&differing, Some(&strings));
    assert!(
        matches!(violations.as_slice(), [BuildViolation::Command(v)] if v.rule == noctis_rules::RuleId::CMD_04),
        "{violations:?}"
    );
}
