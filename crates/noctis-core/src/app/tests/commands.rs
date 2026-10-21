//! Story 6.4 Matrix 17-44 (failing sides): every command conflict after
//! merging fails `build()` with `BuildViolation::Command`, Module notes and
//! the scope.

use noctis_rules::command::{IntegrationType, InteractionContext};
use noctis_rules::{Actual, RuleId, Violation};
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::events::MemberAdd;
use crate::metadata::BaseMeta;
use crate::test_support::{command, command_with, leak, leak_paths, message_menu, noop, user_menu};
use crate::{App, BuildError, BuildViolation, CommandMetadata, Module, Runtime};

fn build(modules: impl IntoIterator<Item = Module>) -> Result<App, BuildError> {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    for module in modules {
        builder.module(module);
    }
    builder.build()
}

/// The command violations of a failing build; any other violation fails
/// the test.
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

fn rules(errs: &[Violation]) -> Vec<RuleId> {
    errs.iter().map(|v| v.rule).collect()
}

fn perms(p: Permissions) -> CommandMetadata {
    CommandMetadata::new().default_member_permissions(p)
}

/// Matrix 17.
#[tokio::test]
async fn meta_01_permissions_conflict_across_modules() {
    let errs = failing([
        Module::new("a").__add(command_with(
            "a::set",
            &["config set"],
            perms(Permissions::MANAGE_GUILD),
        )),
        Module::new("b").__add(command_with(
            "b::get",
            &["config get"],
            perms(Permissions::ADMINISTRATOR),
        )),
    ]);
    assert_eq!(rules(&errs), [RuleId::META_01]);
    assert_eq!(errs[0].path, "config");
    assert_eq!(
        errs[0].notes,
        [
            "`config set` sets `32` (from `a`)",
            "`config get` sets `8` (from `b`)",
            "scope: global"
        ]
    );
}

/// Matrix 18.
#[tokio::test]
async fn meta_01_set_versus_unset_without_base_declaration() {
    let errs = failing([
        Module::new("a").__add(command_with(
            "a::set",
            &["config set"],
            perms(Permissions::MANAGE_GUILD),
        )),
        Module::new("b").__add(command("b::get", &["config get"], "d")),
    ]);
    assert_eq!(rules(&errs), [RuleId::META_01]);
    assert_eq!(errs[0].notes[1], "`config get` leaves it unset (from `b`)");
}

/// Matrix 19.
#[tokio::test]
async fn meta_02_contexts_and_integration_types_conflict() {
    use InteractionContext::{BotDm, Guild};
    let errs = failing([
        Module::new("a").__add(command_with(
            "a::set",
            &["config set"],
            CommandMetadata::new()
                .contexts(&[Guild])
                .integration_types(&[IntegrationType::GuildInstall]),
        )),
        Module::new("b").__add(command_with(
            "b::get",
            &["config get"],
            CommandMetadata::new()
                .contexts(&[Guild, BotDm])
                .integration_types(&[IntegrationType::UserInstall]),
        )),
    ]);
    assert_eq!(rules(&errs), [RuleId::META_02, RuleId::META_02]);
    assert_eq!(errs[0].notes[0], "`config set` sets `[Guild]` (from `a`)");
    assert_eq!(
        errs[1].notes[1],
        "`config get` sets `[UserInstall]` (from `b`)"
    );
}

/// Matrix 20.
#[tokio::test]
async fn cmd_07_nsfw_conflict() {
    let errs = failing([
        Module::new("a").__add(command_with(
            "a::set",
            &["config set"],
            CommandMetadata::new().nsfw(true),
        )),
        Module::new("b").__add(command_with(
            "b::get",
            &["config get"],
            CommandMetadata::new().nsfw(false),
        )),
    ]);
    assert_eq!(rules(&errs), [RuleId::CMD_07]);
    assert_eq!(errs[0].notes.last().unwrap(), "scope: global");
}

/// Matrix 21.
#[tokio::test]
async fn cmd_07_explicit_base_versus_handler() {
    let errs = failing([
        Module::new("a").base("config", BaseMeta::new().nsfw(true)),
        Module::new("b").__add(command_with(
            "b::set",
            &["config set"],
            CommandMetadata::new().nsfw(false),
        )),
    ]);
    assert_eq!(rules(&errs), [RuleId::CMD_07]);
    assert_eq!(
        errs[0].notes,
        [
            "`config` sets `true` (from `a`)",
            "`config set` sets `false` (from `b`)",
            "scope: global"
        ]
    );
}

/// Matrix 22.
#[tokio::test]
async fn reg_04_two_explicit_bases_differ() {
    let errs = failing([
        Module::new("a")
            .__add(command("a::set", &["config set"], "d"))
            .base("config", BaseMeta::new().description("A")),
        Module::new("b").base("config", BaseMeta::new().description("B")),
    ]);
    assert_eq!(rules(&errs), [RuleId::REG_04]);
    assert_eq!(errs[0].path, "config");
    assert_eq!(
        errs[0].notes,
        ["`a` declares `A`", "`b` declares `B`", "scope: global"]
    );
}

/// Matrix 23.
#[tokio::test]
async fn reg_04_group_descriptions_differ() {
    let errs = failing([
        Module::new("a")
            .__add(command("a::prefix", &["config set prefix"], "d"))
            .group("config set", "X"),
        Module::new("b").group("config set", "Y"),
    ]);
    assert_eq!(rules(&errs), [RuleId::REG_04]);
    assert_eq!(errs[0].path, "config set");
}

/// Matrix 24.
#[tokio::test]
async fn meta_02_base_meta_below_the_base() {
    let errs = failing([Module::new("a")
        .__add(command("a::prefix", &["config set prefix"], "d"))
        .base("config set", BaseMeta::new().nsfw(true))]);
    assert_eq!(rules(&errs), [RuleId::META_02]);
    assert_eq!(errs[0].path, "config set");
    assert_eq!(errs[0].help.as_deref(), Some("declare it on `config`"));
    assert_eq!(errs[0].notes, ["declared by `a`", "scope: global"]);
}

/// Matrix 25.
#[tokio::test]
async fn reg_04_orphan_base_declaration() {
    let errs = failing([Module::new("a").base("nothing", BaseMeta::new().description("d"))]);
    assert_eq!(rules(&errs), [RuleId::REG_04]);
    assert_eq!(errs[0].path, "nothing");
    assert_eq!(
        errs[0].limit,
        noctis_rules::Bound::Describe("metadata only for a registered command path")
    );
    assert_eq!(errs[0].notes, ["declared by `a`", "scope: global"]);
}

/// Matrix 26.
#[tokio::test]
async fn cmd_12_invokable_base_across_modules() {
    let errs = failing([
        Module::new("a").__add(command("a::config", &["config"], "d")),
        Module::new("b").__add(command("b::set", &["config set"], "d")),
    ]);
    assert_eq!(rules(&errs), [RuleId::CMD_12]);
    assert_eq!(errs[0].path, "config");
    assert_eq!(
        errs[0].notes,
        [
            "`config` makes `config` invokable (from `a`)",
            "`config set` gives `config` subcommands (from `b`)",
            "scope: global"
        ]
    );
}

/// Matrix 27.
#[tokio::test]
async fn cmd_11_subcommand_and_group_across_modules() {
    let errs = failing([
        Module::new("a").__add(command("a::set", &["config set"], "d")),
        Module::new("b").__add(command("b::prefix", &["config set prefix"], "d")),
    ]);
    assert_eq!(rules(&errs), [RuleId::CMD_11]);
    assert_eq!(errs[0].path, "config set");
    assert_eq!(
        errs[0].notes,
        [
            "`config set` makes `set` a subcommand (from `a`)",
            "`config set prefix` makes `set` a subcommand group (from `b`)",
            "scope: global"
        ]
    );
}

/// Matrix 28.
#[tokio::test]
async fn reg_04_duplicate_path_across_modules() {
    let errs = failing([
        Module::new("a").__add(command("a::set", &["config set"], "d")),
        Module::new("b").__add(command("b::config", &["config set", "config get"], "d")),
    ]);
    assert_eq!(rules(&errs), [RuleId::REG_04]);
    assert_eq!(errs[0].path, "config set");
    assert_eq!(
        errs[0].limit,
        noctis_rules::Bound::Describe("one handler per command path")
    );
    assert_eq!(
        errs[0].notes,
        ["registered by `a`", "registered by `b`", "scope: global"]
    );
}

/// Matrix 29.
#[tokio::test]
async fn reg_04_same_descriptor_twice() {
    let ping = command("a::ping", &["ping"], "d");
    let errs = failing([Module::new("a").__add(ping.clone()).__add(ping)]);
    assert_eq!(rules(&errs), [RuleId::REG_04]);
    assert_eq!(
        errs[0].notes,
        ["registered by `a`", "registered by `a`", "scope: global"]
    );
}

/// Matrix 30.
#[tokio::test]
async fn reg_03_same_user_command_across_modules() {
    let errs = failing([
        Module::new("a").__add(user_menu("a::show", "Show profile", CommandMetadata::new())),
        Module::new("b").__add(user_menu("b::show", "Show profile", CommandMetadata::new())),
    ]);
    assert_eq!(rules(&errs), [RuleId::REG_03]);
    assert_eq!(errs[0].path, "Show profile");
    assert_eq!(
        errs[0].notes,
        ["registered by `a`", "registered by `b`", "scope: global"]
    );
}

/// Matrix 31, 32.
#[tokio::test]
async fn reg_03_byte_equality_and_scopes() {
    const GUILD_1: &[Id<GuildMarker>] = &[Id::new(1)];
    build([
        Module::new("a").__add(user_menu("a::show", "Show profile", CommandMetadata::new())),
        Module::new("b").__add(user_menu("b::show", "Show Profile", CommandMetadata::new())),
    ])
    .unwrap();
    build([
        Module::new("a").__add(user_menu("a::show", "Show profile", CommandMetadata::new())),
        Module::new("b").__add(user_menu(
            "b::show",
            "Show profile",
            CommandMetadata::new().guilds(GUILD_1),
        )),
    ])
    .unwrap();
}

/// Matrix 33.
#[tokio::test]
async fn meta_03_base_meta_on_a_guild_base() {
    const GUILD_42: &[Id<GuildMarker>] = &[Id::new(42)];
    let errs = failing([
        Module::new("a").__add(command_with(
            "a::set",
            &["config set"],
            CommandMetadata::new().guilds(GUILD_42),
        )),
        Module::new("b").base(
            "config",
            BaseMeta::new().contexts(&[InteractionContext::Guild]),
        ),
    ]);
    assert_eq!(rules(&errs), [RuleId::META_03]);
    assert_eq!(errs[0].path, "config.contexts");
    assert_eq!(errs[0].notes, ["declared by `b`", "scope: guild 42"]);
}

/// Matrix 34: every violation at once, setters first.
#[tokio::test]
async fn reg_04_every_violation_at_once() {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .http_timeout(std::time::Duration::ZERO)
        .module(Module::new("a").__add(command_with(
            "a::set",
            &["perm set"],
            perms(Permissions::MANAGE_GUILD),
        )))
        .module(Module::new("b").__add(command_with(
            "b::get",
            &["perm get"],
            perms(Permissions::ADMINISTRATOR),
        )))
        .module(Module::new("c").__add(command("c::inv", &["inv"], "d")))
        .module(Module::new("d").__add(command("d::sub", &["inv sub"], "d")))
        .module(Module::new("e").__add(command("e::dup", &["dup x"], "d")))
        .module(Module::new("f").__add(command("f::dup", &["dup x"], "d")))
        .module(Module::new("g").__add(user_menu("g::show", "Show", CommandMetadata::new())))
        .module(Module::new("h").__add(user_menu("h::show", "Show", CommandMetadata::new())))
        .module(Module::new("i").__add(noop::<MemberAdd>("i::greet")));
    let violations = builder.build().unwrap_err().into_violations();
    assert!(
        matches!(violations[0], BuildViolation::InvalidHttpTimeout { .. }),
        "{violations:?}"
    );
    // `MissingIntent` (fold order) precedes every command violation.
    assert!(
        matches!(
            violations[1],
            BuildViolation::MissingIntent {
                handler: "i::greet",
                ..
            }
        ),
        "{violations:?}"
    );
    let commands: Vec<(RuleId, &str)> = violations[2..]
        .iter()
        .map(|v| match v {
            BuildViolation::Command(v) => (v.rule, v.path.as_str()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        commands,
        [
            (RuleId::REG_04, "dup x"),
            (RuleId::CMD_12, "inv"),
            (RuleId::META_01, "perm"),
            (RuleId::REG_03, "Show"),
        ]
    );
}

/// Matrix 35, 36: a Module description is measured at App build.
#[tokio::test]
async fn cmd_04_module_description_lengths() {
    fn with_base(description: &'static str) -> [Module; 2] {
        [
            Module::new("a").__add(command("a::set", &["config set"], "d")),
            Module::new("b").base("config", BaseMeta::new().description(description)),
        ]
    }
    for (text, len) in [
        (String::new(), 0),
        ("a".repeat(101), 101),
        ("😀".repeat(100), 200),
    ] {
        let errs = failing(with_base(leak(text)));
        assert_eq!(rules(&errs), [RuleId::CMD_04], "{len}");
        assert_eq!(errs[0].actual, Actual::Count(len));
        assert_eq!(
            errs[0].notes,
            [
                "in the default description",
                "declared by `b`",
                "scope: global"
            ]
        );
    }
    for text in ["a".repeat(100), "a".to_owned(), "😀".repeat(50)] {
        build(with_base(leak(text))).unwrap();
    }
}

/// Matrix 37.
#[tokio::test]
async fn cmd_10_group_path_too_deep() {
    let errs = failing([Module::new("a")
        .__add(command("a::c", &["a b c"], "d"))
        .group("a b c d", "d")]);
    assert_eq!(rules(&errs), [RuleId::CMD_10]);
    assert_eq!(errs[0].notes, ["declared by `a`", "scope: global"]);
}

/// `count` handlers `<prefix><i><suffix>`, the first `split` in Module `a`,
/// the rest in `b`.
fn split_modules(
    count: usize,
    split: usize,
    path: impl Fn(usize) -> String,
    metadata: CommandMetadata,
) -> [Module; 2] {
    let mut a = Module::new("a");
    let mut b = Module::new("b");
    for i in 0..count {
        let handler = command("m::h", leak_paths(&path(i)), "d").__with_metadata(metadata);
        if i < split {
            a = a.__add(handler);
        } else {
            b = b.__add(handler);
        }
    }
    [a, b]
}

/// Matrix 38, 39: CMD-13 after merging.
#[tokio::test]
async fn cmd_13_merged_children() {
    let meta = CommandMetadata::new();
    build(split_modules(25, 13, |i| format!("x s{i}"), meta)).unwrap();
    let errs = failing(split_modules(26, 13, |i| format!("x s{i}"), meta));
    assert_eq!(rules(&errs), [RuleId::CMD_13]);
    assert_eq!(errs[0].path, "x");
    assert_eq!(
        errs[0].to_string().lines().next().unwrap(),
        "[CMD-13] x: limit at most 25, actual 26"
    );
    assert_eq!(errs[0].notes, ["contributed by `a`, `b`", "scope: global"]);
    build(split_modules(25, 12, |i| format!("x g s{i}"), meta)).unwrap();
    let errs = failing(split_modules(26, 12, |i| format!("x g s{i}"), meta));
    assert_eq!(rules(&errs), [RuleId::CMD_13]);
    assert_eq!(errs[0].path, "x g");
}

/// A tree of base `x` (explicit description of `base` characters unless
/// `None`), groups `g0`, `g1` (25 subcommands each, Module `a`) and `g2` (24
/// subcommands, Module `b`), every group and subcommand described with 100
/// characters: `1 + base + 2 * (2 + 100 + 25 * 103) + (2 + 100 + 24 * 103)`.
fn sized_tree(base: Option<usize>) -> [Module; 2] {
    let d100 = leak("d".repeat(100));
    let mut a = Module::new("a");
    let mut b = Module::new("b");
    for (group, subs) in [(0, 25), (1, 25), (2, 24)] {
        for s in 0..subs {
            let handler = command("m::h", leak_paths(&format!("x g{group} s{s:02}")), d100);
            if group < 2 {
                a = a.__add(handler);
            } else {
                b = b.__add(handler);
            }
        }
        let path = leak(format!("x g{group}"));
        if group < 2 {
            a = a.group(path, d100);
        } else {
            b = b.group(path, d100);
        }
    }
    if let Some(len) = base {
        a = a.base("x", BaseMeta::new().description(leak("b".repeat(len))));
    }
    [a, b]
}

/// Matrix 40, 41: CMD-05 after merging, fallback descriptions counted.
#[tokio::test]
async fn cmd_05_merged_tree_size() {
    build(sized_tree(Some(71))).unwrap();
    let errs = failing(sized_tree(Some(72)));
    assert_eq!(rules(&errs), [RuleId::CMD_05]);
    assert_eq!(errs[0].path, "x");
    assert_eq!(errs[0].actual, Actual::Count(8001));
    assert_eq!(
        errs[0].notes,
        [
            "longest variant per field",
            "contributed by `a`, `b`",
            "scope: global"
        ]
    );
    // Without a base description the fallback (100 characters) is counted.
    let errs = failing(sized_tree(None));
    assert_eq!(rules(&errs), [RuleId::CMD_05]);
    assert_eq!(errs[0].actual, Actual::Count(8029));
}

/// Matrix 42: 100 global bases pass, 101 fail; a merged base counts once.
#[tokio::test]
async fn reg_01_chat_input_count() {
    let meta = CommandMetadata::new();
    build(split_modules(100, 50, |i| format!("c{i}"), meta)).unwrap();
    let [a, b] = split_modules(100, 50, |i| format!("c{i} x"), meta);
    let extra = Module::new("c").__add(command("c::y", &["c0 y"], "d"));
    build([a, b, extra]).unwrap();
    let errs = failing(split_modules(101, 50, |i| format!("c{i}"), meta));
    assert_eq!(rules(&errs), [RuleId::REG_01]);
    assert_eq!(errs[0].path, "chat-input commands");
    assert_eq!(errs[0].actual, Actual::Count(101));
    assert_eq!(errs[0].notes, ["registered by `a`, `b`", "scope: global"]);
}

fn menus(kind: &str, count: usize, metadata: CommandMetadata) -> Module {
    (0..count).fold(Module::new("a"), |module, i| {
        let name = leak(format!("M{i}"));
        match kind {
            "user" => module.__add(user_menu("a::u", name, metadata)),
            _ => module.__add(message_menu("a::m", name, metadata)),
        }
    })
}

/// Matrix 43: 15 user and 15 message commands pass; 16 fail.
#[tokio::test]
async fn reg_01_menu_counts() {
    let meta = CommandMetadata::new();
    build([menus("user", 15, meta), menus("message", 15, meta)]).unwrap();
    let errs = failing([menus("user", 16, meta)]);
    assert_eq!(rules(&errs), [RuleId::REG_01]);
    assert_eq!(errs[0].path, "user commands");
    assert_eq!(errs[0].notes, ["registered by `a`", "scope: global"]);
    let errs = failing([menus("message", 16, meta)]);
    assert_eq!(errs[0].path, "message commands");
}

/// Matrix 44: guild counts are REG-02 and separate from the global ones.
#[tokio::test]
async fn reg_02_guild_counts() {
    const GUILD_1: &[Id<GuildMarker>] = &[Id::new(1)];
    let guild = CommandMetadata::new().guilds(GUILD_1);
    let errs = failing(split_modules(101, 50, |i| format!("c{i}"), guild));
    assert_eq!(rules(&errs), [RuleId::REG_02]);
    assert_eq!(errs[0].notes, ["registered by `a`, `b`", "scope: guild 1"]);
    let errs = failing([menus("user", 16, guild)]);
    assert_eq!(rules(&errs), [RuleId::REG_02]);
    assert_eq!(errs[0].notes, ["registered by `a`", "scope: guild 1"]);
    let [a, b] = split_modules(100, 100, |i| format!("c{i}"), CommandMetadata::new());
    let [c, d] = split_modules(100, 100, |i| format!("c{i}"), guild);
    build([a, b, c, d]).unwrap();
}

/// Hand-built option structs: 25 options of 103 characters each (name `oNN`
/// plus a 100-character description), or of 4 characters each.
mod sized_options {
    use noctis_rules::command::{OptionDecl, OptionType};

    use crate::options::{CommandInput, OptionError, Options};

    const D100: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddd\
                        dddddddddddddddddddddddddddddddddddddddddddddddddd";

    macro_rules! options {
        ($name:ident, $description:expr) => {
            pub(super) struct $name;

            impl Options for $name {
                const OPTIONS: &'static [OptionDecl<'static>] = &options!(@decls $description;
                    "o00" "o01" "o02" "o03" "o04" "o05" "o06" "o07" "o08" "o09" "o10" "o11"
                    "o12" "o13" "o14" "o15" "o16" "o17" "o18" "o19" "o20" "o21" "o22" "o23"
                    "o24");
                type Partial = crate::autocomplete::NoPartial<Self>;
                fn from_input(_: &CommandInput) -> Result<Self, OptionError> {
                    Ok(Self)
                }
            }
        };
        (@decls $description:expr; $($option:literal)*) => {
            [$(OptionDecl::new($option, OptionType::String, $description)),*]
        };
    }

    options!(Big, D100);
    options!(Small, "d");
}

/// `x s0`, `x s1` in Module `a` and `x s2`, `x s3` in Module `b`, each
/// leaf with 25 options of `O`, under an explicitly described base.
fn optioned_tree<O: crate::options::Options>() -> [Module; 2] {
    fn leaf<O: crate::options::Options>(path: &'static [&'static str]) -> crate::CommandHandler {
        crate::CommandHandler::__new_plain_with_options(
            "m::h",
            path,
            "d",
            |_: crate::CommandCtx, _: crate::CommandResponder, _: O| async {
                Ok(crate::test_support::msg())
            },
        )
    }
    [
        Module::new("a")
            .base("x", BaseMeta::new().description("d"))
            .__add(leaf::<O>(&["x s0"]))
            .__add(leaf::<O>(&["x s1"])),
        Module::new("b")
            .__add(leaf::<O>(&["x s2"]))
            .__add(leaf::<O>(&["x s3"])),
    ]
}

/// CMD-05 counts handler options after merging: each Module's half
/// (`2 * (2 + 1 + 25 * 103)` = 5156) fits, the merged tree (10314) does not.
#[tokio::test]
async fn cmd_05_merged_options_size() {
    build(optioned_tree::<sized_options::Small>()).unwrap();
    for half in optioned_tree::<sized_options::Big>() {
        build([half]).unwrap();
    }
    let errs = failing(optioned_tree::<sized_options::Big>());
    assert_eq!(rules(&errs), [RuleId::CMD_05]);
    assert_eq!(errs[0].path, "x");
    assert_eq!(errs[0].actual, Actual::Count(10314));
    assert_eq!(
        errs[0].notes,
        [
            "longest variant per field",
            "contributed by `a`, `b`",
            "scope: global"
        ]
    );
}
