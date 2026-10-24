//! Story 11.4 Matrix 1-41: command localization in the Publish JSON and its
//! App-build checks (LOC-02, LOC-03, LOC-05, LOC-06).

use noctis_i18n::Catalog;
use noctis_rules::{Locale, RuleId};

use super::*;
use crate::BuildViolation;
use crate::test_support::leak;

/// An en-US-default catalog whose files and entries are sorted as `i18n!`
/// emits them.
fn catalog(origin: &'static str, files: &[(Locale, &[(&str, &str)])]) -> Catalog {
    catalog_in(origin, Locale::EnUs, files)
}

/// [`catalog`] with the default locale `default`.
fn catalog_in(
    origin: &'static str,
    default: Locale,
    files: &[(Locale, &[(&str, &str)])],
) -> Catalog {
    let mut files: Vec<(Locale, Vec<(&'static str, &'static str)>)> = files
        .iter()
        .map(|(locale, entries)| {
            let mut entries: Vec<(&'static str, &'static str)> = entries
                .iter()
                .map(|(k, v)| (leak((*k).to_owned()), leak((*v).to_owned())))
                .collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            (*locale, entries)
        })
        .collect();
    files.sort_by_key(|(locale, _)| *locale);
    let files: Vec<(Locale, &'static [(&'static str, &'static str)])> = files
        .into_iter()
        .map(|(locale, entries)| {
            let entries: &'static [(&'static str, &'static str)] =
                Box::leak(entries.into_boxed_slice());
            (locale, entries)
        })
        .collect();
    Catalog::__new(origin, default, Box::leak(files.into_boxed_slice()), &[])
}

/// `bot/locales` with `files`.
fn bot(files: &[(Locale, &[(&str, &str)])]) -> Catalog {
    catalog("bot/locales", files)
}

fn build(modules: Vec<Module>, catalogs: &[Catalog]) -> Result<App, Vec<BuildViolation>> {
    let mut builder = builder();
    for module in modules {
        builder.module(module);
    }
    for catalog in catalogs {
        builder.locales(*catalog);
    }
    builder.build().map_err(crate::BuildError::into_violations)
}

/// The global JSON of `modules` with `catalogs`, which must build.
fn json_of(modules: Vec<Module>, catalogs: &[Catalog]) -> Vec<Value> {
    let app = build(modules, catalogs).unwrap();
    local_of(&app, Scope::Global)
}

/// The violations of `modules` with `catalogs`, which must fail.
fn violations(modules: Vec<Module>, catalogs: &[Catalog]) -> Vec<BuildViolation> {
    build(modules, catalogs).err().unwrap()
}

fn ban() -> Module {
    Module::new("m").__add(command("m::ban", &["ban"], "Ban a member"))
}

/// The localization violations' `(rule, locale, key, base rule)`.
fn summary(violations: &[BuildViolation]) -> Vec<(RuleId, Locale, &'static str, RuleId)> {
    violations
        .iter()
        .filter_map(|v| match v {
            BuildViolation::CommandLocalization {
                rule,
                locale,
                key,
                violation,
                ..
            } => Some((*rule, *locale, *key, violation.rule)),
            _ => None,
        })
        .collect()
}

/// Story 11.4 Matrix 1, 2: a per-word name, overridden by the full path.
#[tokio::test]
async fn loc_name_per_word_and_full_path() {
    let words = bot(&[
        (Locale::EnUs, &[]),
        (Locale::Tr, &[("cmdword.ban", "yasakla")]),
    ]);
    let json = json_of(vec![ban()], &[words]);
    assert_eq!(
        json,
        [json!({
            "type": 1, "name": "ban", "name_localizations": {"tr": "yasakla"},
            "description": "Ban a member", "options": []
        })]
    );
    let full = bot(&[(
        Locale::Tr,
        &[("cmdword.ban", "yasakla"), ("cmd.ban.name", "engelle")],
    )]);
    let json = json_of(vec![ban()], &[full]);
    assert_eq!(json[0]["name_localizations"], json!({"tr": "engelle"}));
}

/// Story 11.4 Matrix 3, 4: one word serves every node; a full path is
/// scoped to its node.
#[tokio::test]
async fn loc_word_shared_and_full_path_scoped() {
    let modules = || {
        vec![
            Module::new("m")
                .__add(command("m::ban", &["ban"], "Ban"))
                .__add(command("m::modban", &["mod ban"], "Mod ban")),
        ]
    };
    let words = bot(&[(Locale::Tr, &[("cmdword.ban", "yasakla")])]);
    let json = json_of(modules(), &[words]);
    assert_eq!(json[0]["name_localizations"], json!({"tr": "yasakla"}));
    assert_eq!(json[1]["name"], "mod");
    assert!(json[1].get("name_localizations").is_none(), "{}", json[1]);
    assert_eq!(
        json[1]["options"][0]["name_localizations"],
        json!({"tr": "yasakla"})
    );
    let scoped = bot(&[(
        Locale::Tr,
        &[("cmdword.ban", "yasakla"), ("cmd.mod.ban.name", "at")],
    )]);
    let json = json_of(modules(), &[scoped]);
    assert_eq!(json[0]["name_localizations"], json!({"tr": "yasakla"}));
    assert_eq!(
        json[1]["options"][0]["name_localizations"],
        json!({"tr": "at"})
    );
}

/// Story 11.4 Matrix 5: a locale's own per-word value beats its sibling's
/// full-path one.
#[tokio::test]
async fn loc_own_word_beats_sibling_full_path() {
    let strings = bot(&[
        (Locale::EnGb, &[("cmdword.ban", "bar")]),
        (Locale::EnUs, &[("cmd.ban.name", "ban-us")]),
    ]);
    let json = json_of(vec![ban()], &[strings]);
    assert_eq!(
        json[0]["name_localizations"],
        json!({"en-GB": "bar", "en-US": "ban-us"})
    );
}

/// Story 11.4 Matrix 6-10: the per-key sibling chain.
#[tokio::test]
async fn loc_sibling_chain() {
    let us = ("cmd.ban.description", "Ban a member");
    // 6: no en-GB file.
    let json = json_of(vec![ban()], &[bot(&[(Locale::EnUs, &[us])])]);
    assert_eq!(
        json[0]["description_localizations"],
        json!({"en-GB": "Ban a member", "en-US": "Ban a member"})
    );
    // 7: an en-GB file with the key.
    let json = json_of(
        vec![ban()],
        &[bot(&[
            (Locale::EnUs, &[us]),
            (Locale::EnGb, &[("cmd.ban.description", "Ban (GB)")]),
        ])],
    );
    assert_eq!(
        json[0]["description_localizations"],
        json!({"en-GB": "Ban (GB)", "en-US": "Ban a member"})
    );
    // 8: an en-GB file without the key.
    let json = json_of(
        vec![ban()],
        &[bot(&[
            (Locale::EnUs, &[us]),
            (Locale::EnGb, &[("cmdword.kick", "kick")]),
        ])],
    );
    assert_eq!(
        json[0]["description_localizations"]["en-GB"],
        "Ban a member"
    );
    // 9, 10: es-419 serves es-ES; pt-BR has no sibling.
    let json = json_of(
        vec![ban()],
        &[bot(&[(
            Locale::Es419,
            &[("cmd.ban.description", "Banear")],
        )])],
    );
    assert_eq!(
        json[0]["description_localizations"],
        json!({"es-ES": "Banear", "es-419": "Banear"})
    );
}

struct Colors;

const COLOR_CHOICES: &[Choice<'static>] = &[
    Choice::new("Red", ChoiceValue::Str("Red")),
    Choice::new("Blue", ChoiceValue::Str("Blue")),
];
const LEVEL_CHOICES: &[Choice<'static>] = &[
    Choice::new("Minus", ChoiceValue::Int(-1)),
    Choice::new("Half", ChoiceValue::Float(1.5)),
];

impl Options for Colors {
    const OPTIONS: &'static [OptionDecl<'static>] = &[
        OptionDecl::new("c", OptionType::String, "Color").choices(COLOR_CHOICES),
        OptionDecl::new("level", OptionType::Number, "Level").choices(LEVEL_CHOICES),
        OptionDecl::new("çay", OptionType::String, "Tea"),
    ];
    type Partial = crate::autocomplete::NoPartial<Self>;
    fn from_input(_: &CommandInput) -> Result<Self, OptionError> {
        Ok(Self)
    }
}

fn colors(path: &'static [&'static str]) -> Module {
    Module::new("colors").__add(crate::CommandHandler::__new_plain_with_options(
        "m::color",
        path,
        "Pick a color",
        |_: crate::CommandCtx, _: crate::CommandResponder, _: Colors| async { Ok(msg()) },
    ))
}

/// Story 11.4 Matrix 11-14: options, string and integer choices; a float
/// value or a Unicode name has no key.
#[tokio::test]
async fn loc_options_and_choices() {
    let strings = bot(&[(
        Locale::Tr,
        &[
            ("cmd.color.opt.c.name", "renk"),
            ("cmd.color.opt.c.description", "Renk"),
            ("cmd.color.opt.c.choice.Red", "Kırmızı"),
            ("cmd.color.opt.level.choice.-1", "Eksi"),
            ("cmdword.çay", "x"),
        ],
    )]);
    let json = json_of(vec![colors(&["color"])], &[strings]);
    assert_eq!(
        json[0]["options"],
        json!([
            {"type": 3, "name": "c", "name_localizations": {"tr": "renk"},
             "description": "Color", "description_localizations": {"tr": "Renk"},
             "choices": [
                {"name": "Red", "name_localizations": {"tr": "Kırmızı"}, "value": "Red"},
                {"name": "Blue", "value": "Blue"},
             ]},
            {"type": 10, "name": "level", "description": "Level", "choices": [
                {"name": "Minus", "name_localizations": {"tr": "Eksi"}, "value": -1},
                {"name": "Half", "value": 1.5},
            ]},
            {"type": 3, "name": "çay", "description": "Tea"},
        ])
    );
}

/// Story 11.4 Matrix 15, 16: a keyed menu localizes as a whole name.
#[tokio::test]
async fn ctx_01_keyed_menu_localizes() {
    let strings = bot(&[(
        Locale::Tr,
        &[("ctxcmd.show_profile.name", "Profili göster")],
    )]);
    let modules = vec![
        Module::new("m")
            .__add(
                user_menu("m::show", "Show profile", CommandMetadata::new())
                    .__with_key("show_profile"),
            )
            .__add(user_menu("m::other", "Other", CommandMetadata::new())),
    ];
    let json = json_of(modules, &[strings]);
    assert_eq!(
        json,
        [
            json!({"type": 2, "name": "Show profile", "name_localizations": {"tr": "Profili göster"}}),
            json!({"type": 2, "name": "Other"}),
        ]
    );
}

/// Story 11.4 Matrix 17: an empty catalog changes nothing.
#[tokio::test]
async fn loc_empty_catalog_is_byte_identical() {
    let without = json_of(vec![ban(), colors(&["color"])], &[]);
    let with = json_of(
        vec![ban(), colors(&["color"])],
        &[bot(&[(Locale::EnUs, &[])])],
    );
    assert_eq!(
        serde_json::to_string(&without).unwrap(),
        serde_json::to_string(&with).unwrap()
    );
}

/// Story 11.4 Matrix 18, 19: keys from two catalogs; a key in both is
/// `DuplicateLocaleKey` without localization violations.
#[tokio::test]
async fn loc_two_catalogs() {
    let a = catalog("a/locales", &[(Locale::Tr, &[("cmdword.ban", "yasakla")])]);
    let b = catalog(
        "b/locales",
        &[(Locale::Tr, &[("cmd.ban.description", "Yasakla")])],
    );
    let json = json_of(vec![ban()], &[a, b]);
    assert_eq!(json[0]["name_localizations"], json!({"tr": "yasakla"}));
    assert_eq!(
        json[0]["description_localizations"],
        json!({"tr": "Yasakla"})
    );
    let bad = catalog("c/locales", &[(Locale::Tr, &[("cmdword.ban", "Yasakla")])]);
    let found = violations(vec![ban()], &[a, bad]);
    assert!(summary(&found).is_empty(), "{found:?}");
    assert!(
        matches!(
            found.as_slice(),
            [BuildViolation::DuplicateLocaleKey {
                key: "cmdword.ban",
                ..
            }]
        ),
        "{found:?}"
    );
}

/// Story 11.4 Matrix 20-22: a base without a declared description takes
/// the catalog's default-locale text; a declared one equal to it is kept.
#[tokio::test]
async fn cmd_04_base_description_from_catalog() {
    let strings = bot(&[
        (Locale::EnUs, &[("cmd.setup.description", "Set the bot up")]),
        (Locale::Tr, &[("cmd.setup.description", "Botu kur")]),
    ]);
    let setup = || Module::new("m").__add(command("m::start", &["setup start"], "Start"));
    let json = json_of(vec![setup()], &[strings]);
    assert_eq!(json[0]["description"], "Set the bot up");
    assert_eq!(
        json[0]["description_localizations"],
        json!({"en-GB": "Set the bot up", "en-US": "Set the bot up", "tr": "Botu kur"})
    );
    let declared = setup().base("setup", BaseMeta::new().description("Set the bot up"));
    let json = json_of(vec![declared], &[strings]);
    assert_eq!(json[0]["description"], "Set the bot up");
    let declared = setup().base("setup", BaseMeta::new().description("X"));
    let json = json_of(vec![declared], &[]);
    assert_eq!(json[0]["description"], "X");
    let json = json_of(vec![setup()], &[]);
    assert_eq!(json[0]["description"], "Start");
}

/// Story 11.4 Matrix 23: the guild mirror keeps the localizations.
#[tokio::test]
async fn loc_guild_mirror_keeps_localizations() {
    let strings = bot(&[(Locale::Tr, &[("cmdword.ban", "yasakla")])]);
    let app = build(vec![ban()], &[strings]).unwrap();
    let mirror = targets(
        app.registry().commands(),
        PublishScope::Guild(Id::new(42)),
        false,
    )
    .unwrap();
    assert_eq!(
        mirror[0].local[0]["name_localizations"],
        json!({"tr": "yasakla"})
    );
}

/// Story 11.4 Matrix 24: the diff compares the maps.
#[tokio::test]
async fn loc_diff_compares_localizations() {
    let strings = bot(&[(Locale::Tr, &[("cmdword.ban", "yasakla")])]);
    let local = json_of(vec![ban()], &[strings]);
    assert_eq!(diff_of(&local, &local), diff::Diff::default());
    let remote = json_of(vec![ban()], &[]);
    let found = diff_of(&local, &remote);
    assert_eq!(found.lines.len(), 1, "{found:?}");
    assert!(
        found.lines[0].contains("~ update") && found.lines[0].contains("name_localizations"),
        "{found:?}"
    );
}

/// Story 11.4 Matrix 25, 26: localized names follow CMD-01..03.
#[tokio::test]
async fn loc_02_name_rules() {
    let found = violations(
        vec![ban()],
        &[bot(&[(Locale::Tr, &[("cmdword.ban", "Yasakla")])])],
    );
    assert_eq!(found.len(), 1, "{found:?}");
    let BuildViolation::CommandLocalization {
        rule,
        locale,
        key,
        origin,
        violation,
    } = &found[0]
    else {
        panic!("{found:?}");
    };
    assert_eq!(
        (*rule, *locale, *key, *origin, violation.rule),
        (
            RuleId::LOC_02,
            Locale::Tr,
            "cmdword.ban",
            "bot/locales",
            RuleId::CMD_03
        )
    );
    assert_eq!(
        found[0].to_string(),
        "[LOC-02] locale `tr`, key `cmdword.ban` (catalog `bot/locales`): [CMD-03] limit \
         lowercase, actual `Yasakla`\n  = help: use `yasakla`"
    );
    for bad in ["ya sakla", "it's"] {
        let found = violations(
            vec![ban()],
            &[bot(&[(Locale::Tr, &[("cmd.ban.name", bad)])])],
        );
        assert_eq!(
            summary(&found),
            [(RuleId::LOC_02, Locale::Tr, "cmd.ban.name", RuleId::CMD_01)],
            "{bad}"
        );
    }
}

/// Story 11.4 Matrix 27, 28: name length bounds; Unicode names pass.
#[tokio::test]
async fn loc_02_name_bounds_and_unicode() {
    for (text, ok) in [
        (String::new(), false),
        ("a".repeat(33), false),
        ("a".repeat(32), true),
        ("kullanıcı".to_owned(), true),
        ("नमस्ते".to_owned(), true),
    ] {
        let strings = bot(&[(Locale::Tr, &[("cmd.ban.name", text.as_str())])]);
        let result = build(vec![ban()], &[strings]);
        if ok {
            assert!(result.is_ok(), "{text}");
        } else {
            assert_eq!(
                summary(&result.err().unwrap()),
                [(RuleId::LOC_02, Locale::Tr, "cmd.ban.name", RuleId::CMD_02)],
                "{text}"
            );
        }
    }
}

/// Story 11.4 Matrix 29, 30: descriptions follow CMD-04 and choice names
/// OPT-09, counted in UTF-16 units.
#[tokio::test]
async fn loc_02_description_and_choice_bounds() {
    for (text, ok) in [
        (String::new(), false),
        ("a".repeat(101), false),
        ("a".repeat(100), true),
        ("😀".repeat(50), true),
        ("😀".repeat(51), false),
    ] {
        let strings = bot(&[(
            Locale::Tr,
            &[
                ("cmd.color.description", text.as_str()),
                ("cmd.color.opt.c.choice.Red", text.as_str()),
            ],
        )]);
        let result = build(vec![colors(&["color"])], &[strings]);
        if ok {
            assert!(result.is_ok(), "{}", text.len());
        } else {
            assert_eq!(
                summary(&result.err().unwrap()),
                [
                    (
                        RuleId::LOC_02,
                        Locale::Tr,
                        "cmd.color.description",
                        RuleId::CMD_04
                    ),
                    (
                        RuleId::LOC_02,
                        Locale::Tr,
                        "cmd.color.opt.c.choice.Red",
                        RuleId::OPT_09
                    ),
                ],
                "{}",
                text.len()
            );
        }
    }
}

/// Story 11.4 Matrix 31: localized menu names follow CTX-01.
#[tokio::test]
async fn ctx_01_localized_menu_names() {
    let menu =
        || {
            vec![Module::new("m").__add(
                user_menu("m::show", "Show profile", CommandMetadata::new()).__with_key("show"),
            )]
        };
    for (text, ok) in [
        ("a".repeat(33), false),
        ("a".repeat(32), true),
        ("Profili Göster".to_owned(), true),
    ] {
        let strings = bot(&[(Locale::Tr, &[("ctxcmd.show.name", text.as_str())])]);
        let result = build(menu(), &[strings]);
        if ok {
            assert!(result.is_ok(), "{text}");
        } else {
            assert_eq!(
                summary(&result.err().unwrap()),
                [(
                    RuleId::LOC_02,
                    Locale::Tr,
                    "ctxcmd.show.name",
                    RuleId::CTX_01
                )]
            );
        }
    }
}

/// Story 11.4 Matrix 32, 33: a sibling-derived or shared value is reported
/// once, naming the file's locale.
#[tokio::test]
async fn loc_02_reported_once() {
    let found = violations(
        vec![ban()],
        &[bot(&[(Locale::EnUs, &[("cmd.ban.description", "")])])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_02,
            Locale::EnUs,
            "cmd.ban.description",
            RuleId::CMD_04
        )]
    );
    let modules = vec![
        Module::new("m")
            .__add(command("m::ban", &["ban"], "Ban"))
            .__add(command("m::modban", &["mod ban"], "Mod ban")),
    ];
    let found = violations(
        modules,
        &[bot(&[(Locale::Tr, &[("cmdword.ban", "Yasakla")])])],
    );
    assert_eq!(
        summary(&found),
        [(RuleId::LOC_02, Locale::Tr, "cmdword.ban", RuleId::CMD_03)]
    );
}

struct BanOptions;

impl Options for BanOptions {
    const OPTIONS: &'static [OptionDecl<'static>] = &[
        OptionDecl::new("user", OptionType::User, "Who").required(true),
        OptionDecl::new("reason", OptionType::String, "Why"),
    ];
    type Partial = crate::autocomplete::NoPartial<Self>;
    fn from_input(_: &CommandInput) -> Result<Self, OptionError> {
        Ok(Self)
    }
}

fn ban_with_options() -> Module {
    Module::new("m").__add(crate::CommandHandler::__new_plain_with_options(
        "m::ban",
        &["ban"],
        "Ban a member",
        |_: crate::CommandCtx, _: crate::CommandResponder, _: BanOptions| async { Ok(msg()) },
    ))
}

/// Story 11.4 Matrix 34-36, 38: localized option names must be distinct
/// per locale.
#[tokio::test]
async fn loc_03_option_names() {
    // 34: equals another option's default.
    let found = violations(
        vec![ban_with_options()],
        &[bot(&[(Locale::Tr, &[("cmd.ban.opt.reason.name", "user")])])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_03,
            Locale::Tr,
            "cmd.ban.opt.reason.name",
            RuleId::CMD_06
        )]
    );
    // 35: two localized names collide; the later option is reported.
    let found = violations(
        vec![ban_with_options()],
        &[bot(&[(
            Locale::Tr,
            &[
                ("cmd.ban.opt.user.name", "x"),
                ("cmd.ban.opt.reason.name", "x"),
            ],
        )])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_03,
            Locale::Tr,
            "cmd.ban.opt.reason.name",
            RuleId::CMD_06
        )]
    );
    // 36: equal to its own default.
    assert!(
        build(
            vec![ban_with_options()],
            &[bot(&[(Locale::Tr, &[("cmd.ban.opt.user.name", "user")])])]
        )
        .is_ok()
    );
    // 38: equal names in different locales.
    assert!(
        build(
            vec![ban_with_options()],
            &[bot(&[
                (Locale::De, &[("cmd.ban.opt.user.name", "x")]),
                (Locale::Tr, &[("cmd.ban.opt.reason.name", "x")]),
            ])]
        )
        .is_ok()
    );
}

/// Story 11.4 Matrix 37: localized subcommand names must be distinct.
#[tokio::test]
async fn loc_03_subcommand_names() {
    let modules = vec![
        Module::new("m")
            .__add(command("m::ban", &["mod ban"], "Ban"))
            .__add(command("m::kick", &["mod kick"], "Kick")),
    ];
    let found = violations(
        modules,
        &[bot(&[(Locale::Tr, &[("cmd.mod.ban.name", "kick")])])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_03,
            Locale::Tr,
            "cmd.mod.ban.name",
            RuleId::CMD_06
        )]
    );
}

/// Story 11.4 Matrix 39: the CMD-05 budget counts the longest variant.
#[tokio::test]
async fn cmd_05_counts_the_longest_localization() {
    // `big` (3 + a 97-character fallback description), 7 groups `gNN` (the
    // same) and 72 subcommands `sNN` (3 + 97): 80 nodes of 100 = 8000, at
    // the limit.
    let description = leak("d".repeat(97));
    let modules = || {
        let mut module = Module::new("m");
        for g in 0..7 {
            for s in 0..(if g == 0 { 12 } else { 10 }) {
                let path = crate::test_support::leak_paths(&format!("big g{g:02} s{s:02}"));
                module = module.__add(command(leak(format!("m::g{g}s{s}")), path, description));
            }
        }
        vec![module]
    };
    assert!(build(modules(), &[]).is_ok());
    let long = "e".repeat(100);
    let strings = bot(&[(
        Locale::Tr,
        &[("cmd.big.g00.s00.description", long.as_str())],
    )]);
    let found = violations(modules(), &[strings]);
    assert_eq!(found.len(), 1, "{found:?}");
    let BuildViolation::Command(violation) = &found[0] else {
        panic!("{found:?}");
    };
    assert_eq!(violation.rule, RuleId::CMD_05);
    assert!(
        violation
            .notes
            .iter()
            .any(|n| n == "longest variant per field"),
        "{violation}"
    );
}

/// Story 11.4 Matrix 40, 41: violations are sorted by key, locale and rule,
/// after the `Command` ones; keys of unregistered commands are ignored.
#[tokio::test]
async fn loc_violation_order() {
    let strings = bot(&[
        (
            Locale::De,
            &[("cmdword.ban", "Ban"), ("cmd.ban.opt.reason.name", "user")],
        ),
        (
            Locale::Tr,
            &[
                ("cmdword.ban", "Yasakla"),
                ("cmd.ban.description", ""),
                ("cmd.ghost.name", "Ghost"),
            ],
        ),
    ]);
    let found = violations(vec![ban_with_options()], &[strings]);
    assert_eq!(
        summary(&found),
        [
            (
                RuleId::LOC_02,
                Locale::Tr,
                "cmd.ban.description",
                RuleId::CMD_04
            ),
            (
                RuleId::LOC_03,
                Locale::De,
                "cmd.ban.opt.reason.name",
                RuleId::CMD_06
            ),
            (RuleId::LOC_02, Locale::De, "cmdword.ban", RuleId::CMD_03),
            (RuleId::LOC_02, Locale::Tr, "cmdword.ban", RuleId::CMD_03),
        ]
    );
    // After the `Command` violations.
    let twice = vec![
        ban_with_options(),
        Module::new("n").__add(command("n::ban", &["ban"], "Ban")),
    ];
    let found = violations(
        twice,
        &[bot(&[(Locale::Tr, &[("cmdword.ban", "Yasakla")])])],
    );
    assert!(matches!(found[0], BuildViolation::Command(_)), "{found:?}");
    assert!(
        matches!(
            found.last(),
            Some(BuildViolation::CommandLocalization { .. })
        ),
        "{found:?}"
    );
}

/// The `BuildViolation::Command`s of `violations`.
fn commands_of(violations: &[BuildViolation]) -> Vec<&noctis_rules::Violation> {
    violations
        .iter()
        .filter_map(|v| match v {
            BuildViolation::Command(violation) => Some(violation),
            _ => None,
        })
        .collect()
}

/// Asserts that `found` is exactly one CMD-04 declared-description
/// violation at `path` for `key`, whose file text is `file` (in the en-US
/// file of `bot/locales`) and whose declaration is by `module`.
fn assert_declared_mismatch(
    found: &[BuildViolation],
    path: &str,
    key: &str,
    file: &str,
    module: &str,
) {
    assert_declared_mismatch_in(found, path, key, file, module, Locale::EnUs);
}

/// [`assert_declared_mismatch`] with the value in `locale`'s file.
fn assert_declared_mismatch_in(
    found: &[BuildViolation],
    path: &str,
    key: &str,
    file: &str,
    module: &str,
    locale: Locale,
) {
    assert_eq!(found.len(), 1, "{found:?}");
    let BuildViolation::Command(violation) = &found[0] else {
        panic!("{found:?}");
    };
    assert_eq!(violation.rule, RuleId::CMD_04, "{violation}");
    assert_eq!(violation.path, path, "{violation}");
    assert_eq!(
        violation.notes,
        [
            format!("description differs from `{key}` in catalog `bot/locales` ({locale})"),
            format!("the file has `{file}`"),
            format!("declared by `{module}`"),
        ],
        "{violation}"
    );
    assert_eq!(
        violation.help.as_deref(),
        Some("make them equal or remove the declaration")
    );
}

/// Story 11.4 Matrix 21 (review pass 1): a declared description must equal
/// its owner catalog's default-locale text (CMD-04 at App-build).
#[tokio::test]
async fn cmd_04_declared_description_differs_from_catalog() {
    let setup_key = ("cmd.setup.description", "Set the bot up");
    // A `Module::base` description.
    let base = Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .base("setup", BaseMeta::new().description("X"));
    let found = violations(vec![base], &[bot(&[(Locale::EnUs, &[setup_key])])]);
    assert_declared_mismatch(
        &found,
        "setup",
        "cmd.setup.description",
        "Set the bot up",
        "m",
    );
    // A `Module::group` description.
    let group = Module::new("m")
        .__add(command("m::add", &["role grp add"], "Add"))
        .group("role grp", "Groups");
    let found = violations(
        vec![group],
        &[bot(&[(
            Locale::EnUs,
            &[("cmd.role.grp.description", "Role groups")],
        )])],
    );
    assert_declared_mismatch(
        &found,
        "role grp",
        "cmd.role.grp.description",
        "Role groups",
        "m",
    );
    // A file-less handler literal.
    let found = violations(
        vec![ban()],
        &[bot(&[(Locale::EnUs, &[("cmd.ban.description", "Ban!")])])],
    );
    assert_declared_mismatch(&found, "ban", "cmd.ban.description", "Ban!", "m");
    // One path of a Pattern handler with per-path descriptions.
    let pattern = Module::new("m").__add(
        command("m::config", &["config set", "config get"], "Set it")
            .__with_descriptions(&["Set it", "Get it"]),
    );
    let found = violations(
        vec![pattern],
        &[bot(&[(
            Locale::EnUs,
            &[
                ("cmd.config.set.description", "Set it"),
                ("cmd.config.get.description", "Read it"),
            ],
        )])],
    );
    assert_declared_mismatch(
        &found,
        "config get",
        "cmd.config.get.description",
        "Read it",
        "m",
    );
}

/// Story 11.4 Matrix 21 (review pass 1): a runtime option description, a
/// node in two scopes and the cases that give no violation.
#[tokio::test]
async fn cmd_04_declared_description_differs_from_catalog_options_and_scopes() {
    const GUILDS: &[Id<GuildMarker>] = &[Id::new(42)];
    let setup_key = ("cmd.setup.description", "Set the bot up");
    // A runtime option description.
    let found = violations(
        vec![ban_with_options()],
        &[bot(&[(
            Locale::EnUs,
            &[("cmd.ban.opt.user.description", "Target")],
        )])],
    );
    assert_declared_mismatch(
        &found,
        "ban.user",
        "cmd.ban.opt.user.description",
        "Target",
        "m",
    );
    // The same node in the global and a guild scope: one violation.
    let scoped = Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .__add(command_with(
            "m::stop",
            &["setup stop"],
            CommandMetadata::new().guilds(GUILDS),
        ))
        .base("setup", BaseMeta::new().description("X"));
    let found = violations(vec![scoped], &[bot(&[(Locale::EnUs, &[setup_key])])]);
    assert_declared_mismatch(
        &found,
        "setup",
        "cmd.setup.description",
        "Set the bot up",
        "m",
    );
    // An equal declaration (Matrix 21) and a key only in a non-default
    // locale give none.
    let equal = Module::new("m")
        .__add(command("m::start", &["setup start"], "Start"))
        .base("setup", BaseMeta::new().description("Set the bot up"));
    assert!(build(vec![equal], &[bot(&[(Locale::EnUs, &[setup_key])])]).is_ok());
    assert!(
        build(
            vec![ban()],
            &[bot(&[(Locale::Tr, &[("cmd.ban.description", "Yasakla")])])]
        )
        .is_ok()
    );
    // A failing catalog merge gives none.
    let a = catalog(
        "a/locales",
        &[(Locale::EnUs, &[("cmd.ban.description", "A")])],
    );
    let b = catalog(
        "b/locales",
        &[(Locale::EnUs, &[("cmd.ban.description", "B")])],
    );
    let found = violations(vec![ban()], &[a, b]);
    assert!(commands_of(&found).is_empty(), "{found:?}");
    assert!(
        matches!(
            found.as_slice(),
            [BuildViolation::DuplicateLocaleKey { .. }]
        ),
        "{found:?}"
    );
}

/// The Story 11.4 Matrix 39 fixture: `big` and 7 groups `gNN` (3 + a
/// 97-character fallback description each) and 72 subcommands `sNN` (3 +
/// 97): 80 nodes of 100 = 8000, at the CMD-05 limit. `last` replaces the
/// handler of `big g06 s09` (no fallback description comes from it).
fn at_the_limit(last: Option<crate::CommandHandler>) -> Vec<Module> {
    let description = leak("d".repeat(97));
    let mut module = Module::new("m");
    let mut last = last;
    for g in 0..7 {
        for s in 0..(if g == 0 { 12 } else { 10 }) {
            let path = crate::test_support::leak_paths(&format!("big g{g:02} s{s:02}"));
            let replaced = if (g, s) == (6, 9) { last.take() } else { None };
            let handler = match replaced {
                Some(handler) => handler,
                None => command(leak(format!("m::g{g}s{s}")), path, description),
            };
            module = module.__add(handler);
        }
    }
    vec![module]
}

/// The single CMD-05 `Command` violation of `found`.
fn assert_cmd_05(found: &[BuildViolation]) {
    assert_eq!(found.len(), 1, "{found:?}");
    let BuildViolation::Command(violation) = &found[0] else {
        panic!("{found:?}");
    };
    assert_eq!(violation.rule, RuleId::CMD_05, "{violation}");
}

/// Story 11.4 Matrix 39 (review pass 1): the CMD-05 budget counts localized
/// names and option texts; a LOC-02-invalid value only gives LOC-02.
#[tokio::test]
async fn cmd_05_counts_localized_names_and_options() {
    assert!(build(at_the_limit(None), &[]).is_ok());
    // A 32-character `tr` name only.
    let name = "a".repeat(32);
    let strings = bot(&[(Locale::Tr, &[("cmdword.big", name.as_str())])]);
    assert_cmd_05(&violations(at_the_limit(None), &[strings]));
    // A localized option description on an options-bearing handler that
    // keeps the fixture at the limit: `user` + `Who` and `reason` + `Why`
    // are 16, so its description is 81 long.
    let size = noctis_rules::command::measure_options(BanOptions::OPTIONS, &Limits::default());
    assert_eq!(size, 16);
    let with_options = || {
        at_the_limit(Some(crate::CommandHandler::__new_plain_with_options(
            "m::last",
            &["big g06 s09"],
            leak("d".repeat(81)),
            |_: crate::CommandCtx, _: crate::CommandResponder, _: BanOptions| async { Ok(msg()) },
        )))
    };
    assert!(build(with_options(), &[]).is_ok());
    let strings = bot(&[(
        Locale::Tr,
        &[("cmd.big.g06.s09.opt.user.description", "Yasaklanacak üye")],
    )]);
    assert_cmd_05(&violations(with_options(), &[strings]));
    // An over-long, LOC-02-invalid name: LOC-02 only, no CMD-05.
    let long = "a".repeat(40);
    let strings = bot(&[(Locale::Tr, &[("cmdword.big", long.as_str())])]);
    let found = violations(at_the_limit(None), &[strings]);
    assert!(commands_of(&found).is_empty(), "{found:?}");
    assert_eq!(
        summary(&found),
        [(RuleId::LOC_02, Locale::Tr, "cmdword.big", RuleId::CMD_02)]
    );
}

/// Story 11.4 Matrix 37 (review pass 1): LOC-03 over a subcommand group's
/// children array.
#[tokio::test]
async fn loc_03_group_children() {
    let modules = vec![
        Module::new("m")
            .__add(command("m::a", &["mod grp a"], "A"))
            .__add(command("m::b", &["mod grp b"], "B")),
    ];
    let found = violations(
        modules,
        &[bot(&[(Locale::Tr, &[("cmd.mod.grp.a.name", "b")])])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_03,
            Locale::Tr,
            "cmd.mod.grp.a.name",
            RuleId::CMD_06
        )]
    );
}

/// Story 11.4 Matrix 37 (review pass 1): LOC-03 over a children array that
/// only a guild scope has.
#[tokio::test]
async fn loc_03_guild_only_child() {
    const GUILDS: &[Id<GuildMarker>] = &[Id::new(42)];
    let modules = || {
        vec![
            Module::new("m")
                .__add(command("m::ban", &["mod ban"], "Ban"))
                .__add(
                    command("m::gban", &["mod ban"], "Ban")
                        .__with_metadata(CommandMetadata::new().guilds(GUILDS)),
                )
                .__add(
                    command("m::kick", &["mod kick"], "Kick")
                        .__with_metadata(CommandMetadata::new().guilds(GUILDS)),
                ),
        ]
    };
    assert!(build(modules(), &[]).is_ok());
    let found = violations(
        modules(),
        &[bot(&[(Locale::Tr, &[("cmd.mod.ban.name", "kick")])])],
    );
    assert_eq!(
        summary(&found),
        [(
            RuleId::LOC_03,
            Locale::Tr,
            "cmd.mod.ban.name",
            RuleId::CMD_06
        )]
    );
}

/// Story 11.4 Matrix M6 (review pass 1): a Pattern handler's per-path
/// descriptions reach the Registry and the Publish JSON.
#[tokio::test]
async fn loc_pattern_per_path_descriptions() {
    let pattern = || {
        Module::new("m").__add(
            command("m::config", &["config set", "config get"], "Set it")
                .__with_descriptions(&["Set it", "Get it"]),
        )
    };
    let app = build(vec![pattern()], &[]).unwrap();
    let scope = app.registry().commands().scope(Scope::Global).unwrap();
    let base = &scope.bases[0];
    let described: Vec<(&str, &str)> = base
        .children
        .iter()
        .map(|c| (c.path, c.description))
        .collect();
    assert_eq!(
        described,
        [("config set", "Set it"), ("config get", "Get it")]
    );
    let json = local_of(&app, Scope::Global);
    assert_eq!(json[0]["options"][0]["description"], "Set it");
    assert_eq!(json[0]["options"][1]["description"], "Get it");
    let strings = bot(&[(
        Locale::Tr,
        &[
            ("cmd.config.set.description", "Ayarla"),
            ("cmd.config.get.description", "Oku"),
        ],
    )]);
    let json = json_of(vec![pattern()], &[strings]);
    assert_eq!(
        json[0]["options"][0]["description_localizations"],
        json!({"tr": "Ayarla"})
    );
    assert_eq!(
        json[0]["options"][1]["description_localizations"],
        json!({"tr": "Oku"})
    );
}

/// Story 11.4 Matrix 21 (review pass 2): the default locale's value can
/// come from its sibling file; a declaration or handler fallback that
/// differs from it is CMD-04, a non-sibling locale is never compared.
#[tokio::test]
async fn cmd_04_declared_description_differs_from_sibling_default() {
    let gb_text = "Set the bot up (GB)";
    let strings = || {
        bot(&[
            (Locale::EnUs, &[("cmd.other.description", "Other")]),
            (Locale::EnGb, &[("cmd.setup.description", gb_text)]),
        ])
    };
    let setup = || Module::new("m").__add(command("m::start", &["setup start"], "Start"));
    // (a) A differing `Module::base` description.
    let found = violations(
        vec![setup().base("setup", BaseMeta::new().description("X"))],
        &[strings()],
    );
    assert_declared_mismatch_in(
        &found,
        "setup",
        "cmd.setup.description",
        gb_text,
        "m",
        Locale::EnGb,
    );
    // (b) An equal one.
    let json = json_of(
        vec![setup().base("setup", BaseMeta::new().description(gb_text))],
        &[strings()],
    );
    assert_eq!(json[0]["description"], gb_text);
    assert_eq!(json[0]["description_localizations"]["en-US"], gb_text);
    // (c) No declaration: the handler fallback differs.
    let found = violations(vec![setup()], &[strings()]);
    assert_declared_mismatch_in(
        &found,
        "setup",
        "cmd.setup.description",
        gb_text,
        "m",
        Locale::EnGb,
    );
    // (d) A key only in a non-sibling locale.
    let tr_only = bot(&[(Locale::Tr, &[("cmd.setup.description", "Botu kur")])]);
    assert!(
        build(
            vec![setup().base("setup", BaseMeta::new().description("X"))],
            &[tr_only]
        )
        .is_ok()
    );
    // An option description only in the en-GB file.
    let found = violations(
        vec![ban_with_options()],
        &[bot(&[(
            Locale::EnGb,
            &[("cmd.ban.opt.user.description", "Target")],
        )])],
    );
    assert_declared_mismatch_in(
        &found,
        "ban.user",
        "cmd.ban.opt.user.description",
        "Target",
        "m",
        Locale::EnGb,
    );
}

/// Story 11.4 Matrix 20, 21 (review pass 2): a catalog whose default
/// locale is `tr` supplies and checks descriptions from its `tr` file.
#[tokio::test]
async fn cmd_04_tr_default_catalog() {
    let strings = || {
        catalog_in(
            "bot/locales",
            Locale::Tr,
            &[(Locale::Tr, &[("cmd.setup.description", "Botu kur")])],
        )
    };
    let setup = || Module::new("m").__add(command("m::start", &["setup start"], "Start"));
    // `default_description`: the `tr` text becomes `description`.
    let json = json_of(vec![setup()], &[strings()]);
    assert_eq!(json[0]["description"], "Botu kur");
    assert_eq!(
        json[0]["description_localizations"],
        json!({"tr": "Botu kur"})
    );
    // `declared_descriptions`: a differing declaration names `(tr)`.
    let found = violations(
        vec![setup().base("setup", BaseMeta::new().description("X"))],
        &[strings()],
    );
    assert_declared_mismatch_in(
        &found,
        "setup",
        "cmd.setup.description",
        "Botu kur",
        "m",
        Locale::Tr,
    );
}

/// Story 11.4 Matrix 40 (review pass 2): the declared-description CMD-04s
/// follow `validate_merge`'s `Command`s, sorted by key, and precede every
/// `CommandLocalization`.
#[tokio::test]
async fn cmd_04_declared_description_order() {
    let strings = bot(&[
        (
            Locale::EnUs,
            &[
                ("cmd.setup.description", "Set the bot up"),
                ("cmd.ban.description", "Ban!"),
            ],
        ),
        (Locale::Tr, &[("cmdword.ban", "Yasakla")]),
    ]);
    let modules = vec![
        Module::new("s")
            .__add(command("s::start", &["setup start"], "Start"))
            .base("setup", BaseMeta::new().description("X")),
        ban(),
        Module::new("n").__add(command("n::ban", &["ban"], "Ban!")),
    ];
    let found = violations(modules, &[strings]);
    let kinds: Vec<(&str, Option<RuleId>, String)> = found
        .iter()
        .map(|v| match v {
            BuildViolation::Command(v) => ("Command", Some(v.rule), v.path.clone()),
            BuildViolation::CommandLocalization { key, .. } => {
                ("CommandLocalization", None, (*key).to_owned())
            }
            other => panic!("{other:?}"),
        })
        .collect();
    // The duplicate `ban` gives `validate_merge`'s REG-04s first.
    let (merge, rest) = kinds.split_at(kinds.len() - 3);
    assert!(!merge.is_empty(), "{found:?}");
    assert!(
        merge
            .iter()
            .all(|(kind, rule, _)| *kind == "Command" && *rule == Some(RuleId::REG_04)),
        "{found:?}"
    );
    assert_eq!(
        rest,
        [
            ("Command", Some(RuleId::CMD_04), "ban".to_owned()),
            ("Command", Some(RuleId::CMD_04), "setup".to_owned()),
            ("CommandLocalization", None, "cmdword.ban".to_owned()),
        ],
        "{found:?}"
    );
}
