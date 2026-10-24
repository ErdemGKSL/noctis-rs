//! Story 6.9 unit tests: the Publish JSON (Matrix 1-5), the canonical diff
//! (Matrix 7-14), the targets and their checks (Matrix 6, 16-19).

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use noctis_rules::command::{
    Choice, ChoiceValue, InteractionContext, NumericValue, OptionDecl, OptionType,
};
use serde_json::json;
use twilight_model::guild::Permissions;

use super::*;
use crate::metadata::{BaseMeta, CommandMetadata};
use crate::module::Module;
use crate::options::{CommandInput, OptionError, Options};
use crate::test_support::{command, command_with, message_menu, msg, user_menu};
use crate::{AppBuilder, Runtime};

fn builder() -> AppBuilder {
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    builder
}

fn app(modules: Vec<Module>) -> App {
    let mut builder = builder();
    for module in modules {
        builder.module(module);
    }
    builder.build().unwrap()
}

/// The local JSON of `scope`.
fn local_of(app: &App, scope: Scope) -> Vec<Value> {
    app.registry()
        .commands()
        .scope(scope)
        .map(json::local)
        .unwrap_or_default()
}

struct Opts;

const CHOICES_STR: &[Choice<'static>] = &[Choice::new("Alpha", ChoiceValue::Str("a"))];
const CHOICES_INT: &[Choice<'static>] = &[Choice::new("One", ChoiceValue::Int(1))];
const CHOICES_FLOAT: &[Choice<'static>] = &[Choice::new("Half", ChoiceValue::Float(0.5))];

impl Options for Opts {
    const OPTIONS: &'static [OptionDecl<'static>] = &[
        OptionDecl::new("count", OptionType::Integer, "Çalış 😀")
            .required(true)
            .min_value(NumericValue::Int(1))
            .max_value(NumericValue::Int(10)),
        OptionDecl::new("ratio", OptionType::Number, "Ratio").min_value(NumericValue::Float(0.5)),
        OptionDecl::new("text", OptionType::String, "Text")
            .max_length(100)
            .choices(CHOICES_STR),
        OptionDecl::new("pick", OptionType::Integer, "Pick").choices(CHOICES_INT),
        OptionDecl::new("num", OptionType::Number, "Num").choices(CHOICES_FLOAT),
        OptionDecl::new("chan", OptionType::Channel, "Chan").channel_types(&[0]),
        OptionDecl::new("query", OptionType::String, "Query").autocomplete(true),
        OptionDecl::new("file", OptionType::Attachment, "File").file_types(&[".pdf"]),
    ];
    type Partial = crate::autocomplete::NoPartial<Self>;
    fn from_input(_: &CommandInput) -> Result<Self, OptionError> {
        Ok(Self)
    }
}

/// Matrix 1.
#[tokio::test]
async fn json_option_less_command() {
    let app = app(vec![Module::new("m").__add(command(
        "m::ping",
        &["ping"],
        "Pong",
    ))]);
    assert_eq!(
        local_of(&app, Scope::Global),
        [json!({"type": 1, "name": "ping", "description": "Pong", "options": []})]
    );
}

/// Matrix 2.
#[tokio::test]
async fn json_metadata_as_declared() {
    let meta = CommandMetadata::new()
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .contexts(&[InteractionContext::Guild])
        .integration_types(&[IntegrationType::GuildInstall])
        .nsfw(false);
    let app = app(vec![Module::new("m").__add(command_with(
        "m::cfg",
        &["cfg"],
        meta,
    ))]);
    assert_eq!(
        local_of(&app, Scope::Global),
        [json!({
            "type": 1, "name": "cfg", "description": "d", "options": [],
            "default_member_permissions": "32", "contexts": [0],
            "integration_types": [0], "nsfw": false,
        })]
    );
}

/// Matrix 3.
#[tokio::test]
async fn json_tree() {
    let app = app(vec![
        Module::new("m")
            .base("config", BaseMeta::new().description("Configure"))
            .group("config set", "Set things")
            .__add(command("m::a", &["config set prefix"], "Set the prefix"))
            .__add(command("m::b", &["config get"], "Get"))
            .__add(command("m::c", &["config show"], "Show")),
    ]);
    assert_eq!(
        local_of(&app, Scope::Global),
        [json!({
            "type": 1, "name": "config", "description": "Configure",
            "options": [
                {"type": 2, "name": "set", "description": "Set things", "options": [
                    {"type": 1, "name": "prefix", "description": "Set the prefix", "options": []},
                ]},
                {"type": 1, "name": "get", "description": "Get", "options": []},
                {"type": 1, "name": "show", "description": "Show", "options": []},
            ],
        })]
    );
}

/// Matrix 4.
#[tokio::test]
async fn json_option_fields() {
    let handler = crate::CommandHandler::__new_plain_with_options(
        "m::h",
        &["opts"],
        "Options",
        |_: crate::CommandCtx, _: crate::CommandResponder, _: Opts| async { Ok(msg()) },
    );
    let app = app(vec![Module::new("m").__add(handler)]);
    let local = local_of(&app, Scope::Global);
    assert_eq!(
        local[0]["options"],
        json!([
            {"type": 4, "name": "count", "description": "Çalış 😀", "required": true,
             "min_value": 1, "max_value": 10},
            {"type": 10, "name": "ratio", "description": "Ratio", "min_value": 0.5},
            {"type": 3, "name": "text", "description": "Text", "max_length": 100,
             "choices": [{"name": "Alpha", "value": "a"}]},
            {"type": 4, "name": "pick", "description": "Pick",
             "choices": [{"name": "One", "value": 1}]},
            {"type": 10, "name": "num", "description": "Num",
             "choices": [{"name": "Half", "value": 0.5}]},
            {"type": 7, "name": "chan", "description": "Chan", "channel_types": [0]},
            {"type": 3, "name": "query", "description": "Query", "autocomplete": true},
            {"type": 11, "name": "file", "description": "File", "file_types": [".pdf"]},
        ])
    );
    // Integers stay JSON integers; unicode survives serialization.
    assert!(local[0]["options"][0]["min_value"].is_i64());
    let text = serde_json::to_string_pretty(&Value::Array(local)).unwrap();
    assert!(text.contains("Çalış 😀"));
}

/// Matrix 5.
#[tokio::test]
async fn json_menus() {
    let app = app(vec![
        Module::new("m")
            .__add(user_menu("m::u", "Report", CommandMetadata::new()))
            .__add(message_menu(
                "m::q",
                "Quote",
                CommandMetadata::new().nsfw(true),
            )),
    ]);
    assert_eq!(
        local_of(&app, Scope::Global),
        [
            json!({"type": 2, "name": "Report"}),
            json!({"type": 3, "name": "Quote", "nsfw": true}),
        ]
    );
}

fn global_defaults() -> Defaults {
    Defaults {
        integration_types: vec![0],
        guild: false,
    }
}

fn canon(values: &[Value], defaults: &Defaults) -> Vec<(diff::Key, Value)> {
    values
        .iter()
        .map(|v| canonical(v, defaults).unwrap())
        .collect()
}

fn diff_of(local: &[Value], remote: &[Value]) -> diff::Diff {
    let defaults = global_defaults();
    diff(&canon(local, &defaults), &canon(remote, &defaults))
}

fn ping() -> Value {
    json!({"type": 1, "name": "ping", "description": "Pong", "options": [
        {"type": 4, "name": "n", "description": "N", "min_value": 1},
        {"type": 3, "name": "s", "description": "S"},
    ]})
}

/// Matrix 7: Discord's server fields and defaults do not count.
#[test]
fn diff_ignores_server_fields_and_defaults() {
    let mut remote = ping();
    let object = remote.as_object_mut().unwrap();
    for (key, value) in [
        ("id", json!("1")),
        ("version", json!("2")),
        ("application_id", json!("7")),
        ("dm_permission", Value::Null),
        ("nsfw", json!(false)),
        ("contexts", Value::Null),
        ("default_member_permissions", Value::Null),
        ("description_localizations", Value::Null),
        ("name_localizations", json!({})),
    ] {
        object.insert(key.into(), value);
    }
    let diff = diff_of(&[ping()], &[remote]);
    assert_eq!(diff, diff::Diff::default());
}

/// Matrix 8: unset contexts equal `[0,1,2]` in any order; unset
/// integration types equal the enabled ones.
#[test]
fn diff_applies_discord_defaults() {
    let mut remote = ping();
    remote["contexts"] = json!([2, 0, 1]);
    remote["integration_types"] = json!([0]);
    assert!(diff_of(&[ping()], &[remote.clone()]).lines.is_empty());

    let defaults = Defaults {
        integration_types: vec![0, 1],
        guild: false,
    };
    let lines = diff(&canon(&[ping()], &defaults), &canon(&[remote], &defaults)).lines;
    assert_eq!(lines, ["~ update chat_input `ping` (integration_types)"]);
}

/// Matrix 9.
#[test]
fn diff_sees_narrower_contexts() {
    let mut remote = ping();
    remote["contexts"] = json!([0, 1]);
    assert_eq!(
        diff_of(&[ping()], &[remote]).lines,
        ["~ update chat_input `ping` (contexts)"]
    );
}

/// Matrix 10 and REG-05: creates, updates and deletes, sorted; the
/// creates are counted.
#[test]
fn reg_05_counts_creates() {
    let cfg = json!({"type": 1, "name": "cfg", "description": "New", "options": []});
    let mut old_cfg = cfg.clone();
    old_cfg["description"] = json!("Old");
    let old = json!({"type": 1, "name": "old", "description": "Old", "options": []});
    let menu = json!({"type": 2, "name": "Report"});
    let diff = diff_of(&[ping(), cfg, menu], &[old, old_cfg]);
    assert_eq!(
        diff.lines,
        [
            "+ create chat_input `ping`",
            "+ create user `Report`",
            "~ update chat_input `cfg` (description)",
            "- delete chat_input `old`",
        ]
    );
    assert_eq!(diff.creates, 2);
}

/// Matrix 11: integer and float bounds compare as numbers.
#[test]
fn diff_compares_numbers_as_f64() {
    let mut remote = ping();
    remote["options"][0]["min_value"] = json!(1.0);
    assert!(diff_of(&[ping()], &[remote]).lines.is_empty());
}

/// Matrix 12: option order matters, command order does not.
#[test]
fn diff_option_order_matters() {
    let mut remote = ping();
    let reversed: Vec<Value> = remote["options"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .cloned()
        .collect();
    remote["options"] = Value::Array(reversed);
    assert_eq!(
        diff_of(&[ping()], &[remote]).lines,
        ["~ update chat_input `ping` (options)"]
    );
    let other = json!({"type": 3, "name": "Quote"});
    let remote_menu = json!({"type": 3, "name": "Quote", "description": ""});
    assert!(
        diff_of(&[ping(), other], &[remote_menu, ping()])
            .lines
            .is_empty()
    );
}

/// Matrix 13: an Entry Point command is not part of the diff.
#[test]
fn diff_skips_other_types() {
    let launch = json!({"type": 4, "name": "launch", "description": "Play", "handler": 2});
    assert!(canonical(&launch, &global_defaults()).is_none());
}

/// Matrix 14.
#[test]
fn diff_empty_sides() {
    assert_eq!(diff_of(&[], &[]), diff::Diff::default());
}

/// Guild targets leave `contexts` and `integration_types` out (META-03).
#[test]
fn diff_guild_ignores_install_fields() {
    let defaults = Defaults {
        integration_types: vec![0, 1],
        guild: true,
    };
    let mut remote = ping();
    remote["integration_types"] = json!([0]);
    remote["contexts"] = Value::Null;
    assert!(
        diff(&canon(&[ping()], &defaults), &canon(&[remote], &defaults))
            .lines
            .is_empty()
    );
}

/// META-04: a declared installation type must be enabled.
#[test]
fn meta_04_rejects_disabled_integration_type() {
    let local = [json!({"type": 1, "name": "me", "integration_types": [0, 1]})];
    let error = check_meta_04(&local, &[0]).unwrap_err();
    assert_eq!(
        error,
        PublishError::IntegrationTypeNotEnabled {
            command: "me".into(),
            integration_type: IntegrationType::UserInstall,
        }
    );
    let text = error.to_string();
    assert!(text.starts_with("[META-04]"), "{text}");
    assert!(
        text.contains("`me`") && text.contains("user_install"),
        "{text}"
    );
    assert!(check_meta_04(&local, &[0, 1]).is_ok());
    assert!(check_meta_04(&[json!({"name": "x"})], &[0]).is_ok());
}

#[test]
fn meta_04_reads_the_application_config() {
    let app =
        parse_application(br#"{"id":"7","integration_types_config":{"0":{},"1":{}}}"#).unwrap();
    assert_eq!((app.id, app.enabled), (7, vec![0, 1]));
    let app = parse_application(br#"{"id":"7"}"#).unwrap();
    assert_eq!(app.enabled, [0]);
    let app = parse_application(br#"{"id":"7","integration_types_config":null}"#).unwrap();
    assert_eq!(app.enabled, [0]);
    assert!(parse_application(br#"{"name":"x"}"#).is_err());
    assert!(parse_application(b"nope").is_err());
}

/// Matrix 6: guild-scoped Commands reach only their guilds.
#[tokio::test]
async fn targets_split_guild_scoped_commands() {
    const GUILDS: &[Id<GuildMarker>] = &[Id::new(42)];
    let app = app(vec![
        Module::new("m")
            .__add(command("m::ping", &["ping"], "Pong"))
            .__add(command_with(
                "m::admin",
                &["admin"],
                CommandMetadata::new().guilds(GUILDS),
            )),
    ]);
    let global = targets(app.registry().commands(), PublishScope::Global, false).unwrap();
    let summary: Vec<(PublishScope, Vec<&str>, bool)> = global
        .iter()
        .map(|t| {
            let names = t.local.iter().filter_map(|c| c["name"].as_str()).collect();
            (t.scope, names, t.meta_04)
        })
        .collect();
    assert_eq!(
        summary,
        [
            (PublishScope::Global, vec!["ping"], true),
            (PublishScope::Guild(Id::new(42)), vec!["admin"], false),
        ]
    );
    let clear = targets(app.registry().commands(), PublishScope::Global, true).unwrap();
    assert_eq!(clear.len(), 1);
    assert!(clear[0].local.is_empty());
}

/// Matrix 16: the dev mirror strips the install fields.
#[tokio::test]
async fn targets_dev_mirror() {
    const GUILDS: &[Id<GuildMarker>] = &[Id::new(42)];
    let meta = CommandMetadata::new()
        .contexts(&[InteractionContext::Guild])
        .integration_types(&[IntegrationType::GuildInstall]);
    let app = app(vec![
        Module::new("m")
            .__add(command_with("m::ping", &["ping"], meta))
            .__add(command_with(
                "m::admin",
                &["admin"],
                CommandMetadata::new().guilds(GUILDS),
            )),
    ]);
    let mirror = targets(
        app.registry().commands(),
        PublishScope::Guild(Id::new(42)),
        false,
    )
    .unwrap();
    assert_eq!(mirror.len(), 1);
    assert_eq!(
        mirror[0].local,
        [
            json!({"type": 1, "name": "ping", "description": "d", "options": []}),
            json!({"type": 1, "name": "admin", "description": "d", "options": []}),
        ]
    );
}

/// Matrix 17.
#[test]
fn reg_03_dev_mirror_name_clash() {
    let local = [
        json!({"type": 1, "name": "ping"}),
        json!({"type": 2, "name": "ping"}),
        json!({"type": 1, "name": "ping"}),
    ];
    let error = check_mirror(Id::new(42), &local).unwrap_err();
    assert_eq!(
        error,
        PublishError::NameClash {
            guild: Id::new(42),
            kind: "chat_input",
            name: "ping".into(),
        }
    );
    assert!(error.to_string().starts_with("[REG-03]"));
}

/// Matrix 18: 100 chat-input and 15 user commands pass, one more fails.
#[test]
fn reg_02_dev_mirror_counts() {
    let chat: Vec<Value> = (0..100)
        .map(|i| json!({"type": 1, "name": format!("c{i}")}))
        .collect();
    assert!(check_mirror(Id::new(1), &chat).is_ok());
    let mut over = chat.clone();
    over.push(json!({"name": "extra"}));
    let error = check_mirror(Id::new(1), &over).unwrap_err();
    assert_eq!(
        error,
        PublishError::TooManyCommands {
            guild: Id::new(1),
            kind: "chat_input",
            count: 101,
            max: 100,
        }
    );
    assert!(error.to_string().starts_with("[REG-02]"));
    let users: Vec<Value> = (0..15)
        .map(|i| json!({"type": 2, "name": format!("u{i}")}))
        .collect();
    assert!(check_mirror(Id::new(1), &users).is_ok());
    let mut over = users;
    over.push(json!({"type": 2, "name": "u15"}));
    assert!(matches!(
        check_mirror(Id::new(1), &over),
        Err(PublishError::TooManyCommands {
            kind: "user",
            count: 16,
            max: 15,
            ..
        })
    ));
}

#[test]
fn report_display_and_counts() {
    let target = TargetReport {
        scope: PublishScope::Global,
        json: "[]".into(),
        diff: "+ create chat_input `ping`".into(),
        creates: 1,
        wrote: false,
    };
    let report = PublishReport {
        targets: vec![target],
        dry_run: true,
    };
    assert_eq!(report.creates(), 1);
    assert!(!report.wrote());
    assert!(report.targets()[0].changed());
    let text = report.to_string();
    assert!(text.contains("== global =="), "{text}");
    assert!(text.contains("+ create chat_input `ping`"), "{text}");
    assert!(text.contains("dry run; nothing written"), "{text}");
    assert!(
        text.ends_with("creates: 1 (REG-05: Discord allows 200 command creates per day per guild)")
    );
}

#[test]
fn errors_name_route_and_code() {
    let error = PublishError::Rejected {
        route: "PUT /applications/7/commands".into(),
        status: 400,
        code: Some(50035),
    };
    let text = error.to_string();
    assert!(
        text.contains("PUT /applications/7/commands") && text.contains("HTTP 400 (code 50035)")
    );
}

mod localize;
