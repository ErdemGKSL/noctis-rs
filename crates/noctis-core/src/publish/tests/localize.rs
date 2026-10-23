//! Story 11.4 Matrix 1-41: command localization in the Publish JSON and its
//! App-build checks (LOC-02, LOC-03, LOC-05, LOC-06).

use noctis_i18n::Catalog;
use noctis_rules::{Locale, RuleId};

use super::*;
use crate::BuildViolation;
use crate::test_support::leak;

/// A catalog whose files and entries are sorted as `i18n!` emits them.
fn catalog(origin: &'static str, files: &[(Locale, &[(&str, &str)])]) -> Catalog {
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
    Catalog::__new(
        origin,
        Locale::EnUs,
        Box::leak(files.into_boxed_slice()),
        &[],
    )
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
    Module::new("m").__add(crate::CommandHandler::__new_plain_with_options(
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
/// the catalog's default-locale text; a declared one wins.
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
    let declared = setup().base("setup", BaseMeta::new().description("X"));
    let json = json_of(vec![declared], &[strings]);
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
