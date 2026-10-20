//! `#[derive(Options)]`, `#[derive(Choices)]` as an option type and
//! `#[command]` with options, end to end through the JSON test seam
//! (Story 6.2, Matrix 1, 2, 9, 27, 30, 37 and 57-79).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::core::__testing::call_command_json;
use noctis::prelude::*;
use noctis::rules::command::{Choice, ChoiceValue, OptionType};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{
    AttachmentMarker, ChannelMarker, GenericMarker, RoleMarker, UserMarker,
};
use noctis::ui::{Mentionable, ResolvedChannel, ResolvedUser};
use noctis::{CommandHandler, HandlerError, InvalidOption, OptionError, UnknownCommandPath};
use serde_json::{Value, json};

#[derive(Options, Debug, Clone, PartialEq)]
struct W {
    #[option(description = "Who to warn")]
    member: ResolvedUser,
    #[option(description = "How long", validate = positive)]
    days: Option<i64>,
    #[option(description = "Why", validate = short)]
    reason: Option<String>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "validators take `&T` of the field type"
)]
fn positive(days: &i64, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if *days > 0 {
        Ok(())
    } else {
        Err(MessageKey::new("warn.days.positive"))
    }
}

#[expect(clippy::ptr_arg, reason = "validators take `&T` of the field type")]
fn short(reason: &String, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if reason.len() < 5 {
        Ok(())
    } else {
        Err(MessageKey::new("warn.reason.short"))
    }
}

static WARNED: Mutex<Vec<W>> = Mutex::new(Vec::new());

#[command("warn", description = "Warn a member")]
async fn warn(_ctx: CommandCtx, options: W) -> HandlerResult<()> {
    WARNED.lock().unwrap().push(options);
    Ok(())
}

#[derive(Options, Debug, Clone, PartialEq)]
struct Empty {}

#[command("mod (ban|kick)", description = "Moderate")]
async fn moderate(_ctx: CommandCtx, path: ModeratePath, options: Kind) -> HandlerResult<()> {
    KINDS.lock().unwrap().push((path, options.severity));
    Ok(())
}

#[derive(Choices, Debug, Clone, Copy, PartialEq)]
enum Severity {
    #[choice("Low")]
    Low,
    #[choice("High", value = "hi")]
    High,
}

#[derive(Options)]
struct Kind {
    #[option(description = "How bad")]
    severity: Severity,
}

static KINDS: Mutex<Vec<(ModeratePath, Severity)>> = Mutex::new(Vec::new());

#[command("ping", description = "Ping")]
async fn ping(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn suggest() {}

#[derive(Options)]
#[expect(dead_code, reason = "only the declarations are read")]
struct Shapes {
    #[option(description = "d")]
    r#type: i64,
    #[option(description = "d", rename = "silent")]
    quiet: bool,
    #[option(description = "d", channel_types = [GuildText, ChannelType::GuildVoice])]
    channel: Id<ChannelMarker>,
    #[option(description = "d", autocomplete = suggest)]
    query: String,
    #[option(description = "d", autocomplete = suggest)]
    count: i64,
    #[option(description = "d", autocomplete = suggest)]
    ratio: f64,
    #[option(description = "d")]
    resolved_channel: Option<ResolvedChannel>,
}

#[derive(Options)]
struct Ids {
    #[option(description = "d")]
    user: Id<UserMarker>,
    #[option(description = "d")]
    channel: Id<ChannelMarker>,
    #[option(description = "d")]
    role: Id<RoleMarker>,
    #[option(description = "d")]
    mentionable: Id<GenericMarker>,
    #[option(description = "d")]
    attachment: Id<AttachmentMarker>,
}

static IDS: Mutex<Vec<[u64; 5]>> = Mutex::new(Vec::new());

#[command("ids", description = "d")]
async fn ids(_ctx: CommandCtx, o: Ids) -> HandlerResult<()> {
    IDS.lock().unwrap().push([
        o.user.get(),
        o.channel.get(),
        o.role.get(),
        o.mentionable.get(),
        o.attachment.get(),
    ]);
    Ok(())
}

#[derive(Options)]
struct Who {
    #[option(description = "d")]
    who: Mentionable,
}

static WHO: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

#[command("who", description = "d")]
async fn who(_ctx: CommandCtx, o: Who) -> HandlerResult<()> {
    let kind = match o.who {
        Mentionable::User(_) => "user",
        Mentionable::Role(_) => "role",
        _ => "other",
    };
    WHO.lock().unwrap().push(kind);
    Ok(())
}

#[derive(Options)]
#[expect(dead_code, reason = "only the decode error is read")]
struct Ratio {
    #[option(description = "d")]
    ratio: f64,
}

#[command("ratio", description = "d")]
async fn ratio(_ctx: CommandCtx, _o: Ratio) -> HandlerResult<()> {
    Ok(())
}

fn user(id: &str) -> Value {
    json!({ "id": id, "username": "u", "discriminator": "0000", "avatar": null, "global_name": null })
}

fn role(id: &str) -> Value {
    json!({
        "id": id, "name": "r", "color": 0, "colors": { "primary_color": 0 }, "hoist": false,
        "managed": false, "mentionable": false, "permissions": "0", "position": 1, "flags": 0
    })
}

fn interaction(name: &str, options: &Value, resolved: Option<&Value>) -> Vec<u8> {
    let mut data = json!({ "name": name, "type": 1, "options": options });
    if let Some(r) = resolved {
        data["resolved"] = r.clone();
    }
    json!({ "type": 2, "data": data }).to_string().into_bytes()
}

fn resolved_member() -> Value {
    json!({
        "users": { "42": user("42") },
        "members": { "42": {
            "roles": [], "joined_at": "2024-01-01T00:00:00.000000+00:00",
            "deaf": false, "mute": false, "flags": 0, "nick": "nick", "pending": false, "permissions": "0"
        }}
    })
}

async fn run(handler: &CommandHandler, json: &[u8]) -> Result<(), HandlerError> {
    call_command_json(handler, json).await
}

fn option_error(err: &HandlerError) -> &OptionError {
    err.as_error()
        .downcast_ref::<OptionError>()
        .unwrap_or_else(|| panic!("not an OptionError: {err}"))
}

/// The warned struct whose `reason` is `marker`; each test sends its own
/// marker, so parallel tests never read each other's pushes.
fn warned(marker: &str) -> Option<W> {
    WARNED
        .lock()
        .unwrap()
        .iter()
        .find(|w| w.reason.as_deref() == Some(marker))
        .cloned()
}

/// Matrix 1: declarations in order, kind by field type.
#[test]
fn options_declarations() {
    let o = W::OPTIONS;
    assert_eq!(o.len(), 3);
    assert_eq!(
        o.iter()
            .map(|d| (d.name, d.kind, d.required))
            .collect::<Vec<_>>(),
        [
            ("member", OptionType::User, true),
            ("days", OptionType::Integer, false),
            ("reason", OptionType::String, false),
        ]
    );
    assert_eq!(o[0].description.default, "Who to warn");
    assert!(o.iter().all(|d| d.choices.is_empty()));
    assert_eq!(warn().options(), W::OPTIONS);
    assert!(Empty::OPTIONS.is_empty());
    assert!(ping().options().is_empty());
}

/// Matrix 9, 27, 30: raw idents, rename, channel types, autocomplete.
#[test]
fn opt_07_names_channel_types_and_autocomplete() {
    let o = Shapes::OPTIONS;
    assert_eq!(o[0].name, "type");
    assert_eq!(o[1].name, "silent");
    assert_eq!(o[2].channel_types, [0, 2]);
    assert_eq!(o[2].kind, OptionType::Channel);
    assert!(o[3].autocomplete && o[4].autocomplete && o[5].autocomplete);
    assert!(!o[6].required);
    assert_eq!(o[6].kind, OptionType::Channel);
}

/// Matrix 37: `derive(Choices)` implements `CommandOption`.
#[test]
fn choices_are_a_string_option() {
    assert_eq!(<Severity as CommandOption>::KIND, OptionType::String);
    assert_eq!(
        <Severity as CommandOption>::CHOICES,
        [
            Choice::new("Low", ChoiceValue::Str("Low")),
            Choice::new("High", ChoiceValue::Str("hi")),
        ]
    );
    assert_eq!(
        <Severity as Choices>::from_value("hi"),
        Some(Severity::High)
    );
    assert_eq!(Kind::OPTIONS[0].choices.len(), 2);
}

/// Matrix 2: an empty struct always decodes.
#[test]
fn empty_struct_decodes() {
    let input = noctis::CommandInput::from_json(&interaction(
        "x",
        &json!([{ "name": "a", "type": 4, "value": 1 }]),
        None,
    ))
    .unwrap();
    assert_eq!(Empty::from_input(&input), Ok(Empty {}));
}

/// Matrix 57, 59, 66: the parsed struct reaches the handler.
#[tokio::test]
async fn parsed_struct_reaches_the_handler() {
    let json = interaction(
        "warn",
        &json!([
            { "name": "member", "type": 6, "value": "42" },
            { "name": "days", "type": 4, "value": 3 },
            { "name": "reason", "type": 3, "value": "p57" },
            { "name": "foo", "type": 3, "value": "ignored" }
        ]),
        Some(&resolved_member()),
    );
    run(&warn(), &json).await.unwrap();
    let got = warned("p57").unwrap();
    assert_eq!(got.member.user.id.get(), 42);
    assert_eq!(
        got.member.member.as_ref().and_then(|m| m.nick.as_deref()),
        Some("nick")
    );
    assert_eq!(got.days, Some(3));
}

/// Matrix 58: a missing required option.
#[tokio::test]
async fn missing_required() {
    let json = interaction(
        "warn",
        &json!([
            { "name": "days", "type": 4, "value": 777 },
            { "name": "reason", "type": 3, "value": "m58" }
        ]),
        None,
    );
    let err = run(&warn(), &json).await.unwrap_err();
    assert_eq!(
        option_error(&err),
        &OptionError::Missing {
            option: "member".into()
        }
    );
    assert_eq!(err.to_string(), "required option `member` is missing");
    assert!(warned("m58").is_none());
}

/// Matrix 60 and 61: wrong types.
#[tokio::test]
async fn opt_02_wrong_type() {
    let json = interaction(
        "warn",
        &json!([
            { "name": "member", "type": 6, "value": "42" },
            { "name": "days", "type": 3, "value": "3" }
        ]),
        Some(&resolved_member()),
    );
    let err = run(&warn(), &json).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "[OPT-02] option `days`: expected INTEGER, found STRING"
    );
    let json = interaction(
        "ratio",
        &json!([{ "name": "ratio", "type": 4, "value": 2 }]),
        None,
    );
    let err = run(&ratio(), &json).await.unwrap_err();
    assert!(matches!(
        option_error(&err),
        OptionError::WrongType {
            expected: OptionType::Number,
            found: OptionType::Integer,
            ..
        }
    ));
}

/// Matrix 50 through the handler: an integer outside `i64`.
#[tokio::test]
async fn opt_03_integer_outside_i64() {
    let json = br#"{"type":2,"data":{"name":"warn","options":[{"name":"member","type":6,"value":"42"},{"name":"days","type":4,"value":9223372036854775808}]}}"#;
    let err = run(&warn(), json).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "[OPT-03] option `days`: 9223372036854775808 is not an i64"
    );
}

/// Matrix 62: an entity missing from the resolved data.
#[tokio::test]
async fn unresolved_entity() {
    let json = interaction(
        "warn",
        &json!([{ "name": "member", "type": 6, "value": "42" }]),
        None,
    );
    let err = run(&warn(), &json).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "option `member`: id 42 is not in the resolved data"
    );
}

/// Matrix 63: plain ids decode without resolved data.
#[tokio::test]
async fn plain_ids() {
    let json = interaction(
        "ids",
        &json!([
            { "name": "user", "type": 6, "value": "1" },
            { "name": "channel", "type": 7, "value": "2" },
            { "name": "role", "type": 8, "value": "3" },
            { "name": "mentionable", "type": 9, "value": "4" },
            { "name": "attachment", "type": 11, "value": "5" }
        ]),
        None,
    );
    run(&ids(), &json).await.unwrap();
    assert!(IDS.lock().unwrap().contains(&[1, 2, 3, 4, 5]));
}

/// Matrix 64: Mentionable prefers a user, then a role.
#[tokio::test]
async fn mentionable() {
    let resolved =
        json!({ "users": { "1": user("1") }, "roles": { "1": role("1"), "2": role("2") } });
    for (id, expected) in [("1", "user"), ("2", "role")] {
        let json = interaction(
            "who",
            &json!([{ "name": "who", "type": 9, "value": id }]),
            Some(&resolved),
        );
        run(&who(), &json).await.unwrap();
        assert_eq!(WHO.lock().unwrap().last(), Some(&expected));
    }
    let json = interaction(
        "who",
        &json!([{ "name": "who", "type": 9, "value": "3" }]),
        Some(&resolved),
    );
    let err = run(&who(), &json).await.unwrap_err();
    assert!(matches!(
        option_error(&err),
        OptionError::Unresolved { id: 3, .. }
    ));
}

/// Matrix 65 and 76: Choices fields with a pattern handler.
#[tokio::test]
async fn choices_and_pattern() {
    let json = |value: Value, ty: u64| {
        json!({ "type": 2, "data": { "name": "mod", "options": [
            { "name": "kick", "type": 1, "options": [{ "name": "severity", "type": ty, "value": value }] }
        ]}})
        .to_string()
        .into_bytes()
    };
    run(&moderate(), &json(json!("hi"), 3)).await.unwrap();
    assert!(
        KINDS
            .lock()
            .unwrap()
            .contains(&(ModeratePath::Kick, Severity::High))
    );
    let err = run(&moderate(), &json(json!("zzz"), 3)).await.unwrap_err();
    assert_eq!(
        option_error(&err),
        &OptionError::UnknownChoice {
            option: "severity".into(),
            value: "zzz".into()
        }
    );
    let err = run(&moderate(), &json(json!(1), 4)).await.unwrap_err();
    assert!(matches!(option_error(&err), OptionError::WrongType { .. }));
    assert_eq!(moderate().options(), Kind::OPTIONS);
}

fn invalid(err: &HandlerError) -> &InvalidOption {
    err.as_error()
        .downcast_ref::<InvalidOption>()
        .unwrap_or_else(|| panic!("not an InvalidOption: {err}"))
}

/// Matrix 67-69 and 71: validators.
#[tokio::test]
async fn validators() {
    let call = |options: Value| interaction("warn", &options, Some(&resolved_member()));
    let member = json!({ "name": "member", "type": 6, "value": "42" });
    run(
        &warn(),
        &call(json!([
            member,
            { "name": "days", "type": 4, "value": 5 },
            { "name": "reason", "type": 3, "value": "v67" }
        ])),
    )
    .await
    .unwrap();
    assert_eq!(warned("v67").and_then(|w| w.days), Some(5));

    let err = run(
        &warn(),
        &call(json!([
            member,
            { "name": "days", "type": 4, "value": -1 },
            { "name": "reason", "type": 3, "value": "v67n" }
        ])),
    )
    .await
    .unwrap_err();
    let e = invalid(&err);
    assert_eq!(e.option(), "days");
    assert_eq!(e.key(), MessageKey::new("warn.days.positive"));
    assert_eq!(
        err.to_string(),
        "option `days` failed validation: warn.days.positive"
    );
    assert!(warned("v67n").is_none());

    // An absent optional value is not validated.
    run(&warn(), &call(json!([member]))).await.unwrap();

    // Declaration order: `days` fails before `reason`.
    let err = run(
        &warn(),
        &call(json!([
            member,
            { "name": "reason", "type": 3, "value": "far too long" },
            { "name": "days", "type": 4, "value": 0 }
        ])),
    )
    .await
    .unwrap_err();
    assert_eq!(invalid(&err).option(), "days");

    // Decoding comes first.
    let err = run(
        &warn(),
        &call(json!([{ "name": "days", "type": 4, "value": -1 }])),
    )
    .await
    .unwrap_err();
    assert!(matches!(option_error(&err), OptionError::Missing { .. }));
}

#[derive(Options)]
struct Renamed {
    #[option(description = "d")]
    r#type: i64,
    #[option(description = "d", rename = "silent", validate = loud)]
    quiet: bool,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "validators take `&T` of the field type"
)]
fn loud(quiet: &bool, _ctx: &CommandCtx) -> Result<(), MessageKey> {
    if *quiet {
        Ok(())
    } else {
        Err(MessageKey::new("renamed.silent"))
    }
}

static RENAMED: Mutex<Vec<(i64, bool)>> = Mutex::new(Vec::new());

#[command("renamed", description = "d")]
async fn renamed(_ctx: CommandCtx, o: Renamed) -> HandlerResult<()> {
    RENAMED.lock().unwrap().push((o.r#type, o.quiet));
    Ok(())
}

/// Matrix 9 at runtime: raw idents and `rename` decode by option name.
#[tokio::test]
async fn raw_ident_and_rename_decode() {
    let json = |ty: i64, silent: bool| {
        interaction(
            "renamed",
            &json!([
                { "name": "type", "type": 4, "value": ty },
                { "name": "silent", "type": 5, "value": silent }
            ]),
            None,
        )
    };
    run(&renamed(), &json(7, true)).await.unwrap();
    assert!(RENAMED.lock().unwrap().contains(&(7, true)));
    let err = run(&renamed(), &json(8, false)).await.unwrap_err();
    assert_eq!(invalid(&err).option(), "silent");
    assert!(!RENAMED.lock().unwrap().contains(&(8, false)));
}

#[derive(Options)]
struct Required {
    #[option(description = "d", validate = positive)]
    count: i64,
}

static REQUIRED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

#[command("required", description = "d")]
async fn required(_ctx: CommandCtx, o: Required) -> HandlerResult<()> {
    REQUIRED.lock().unwrap().push(o.count);
    Ok(())
}

/// Matrix 67 on a required field.
#[tokio::test]
async fn validator_on_required_field() {
    let json = |count: i64| {
        interaction(
            "required",
            &json!([{ "name": "count", "type": 4, "value": count }]),
            None,
        )
    };
    run(&required(), &json(9)).await.unwrap();
    assert!(REQUIRED.lock().unwrap().contains(&9));
    let err = run(&required(), &json(-9)).await.unwrap_err();
    let e = invalid(&err);
    assert_eq!(
        (e.option(), e.key()),
        ("count", MessageKey::new("warn.days.positive"))
    );
    assert!(!REQUIRED.lock().unwrap().contains(&-9));
}

/// Matrix 72-74: the seam's path check, bad JSON and no-options handlers.
#[tokio::test]
async fn seam() {
    let json = interaction("kick", &json!([]), None);
    let err = run(&warn(), &json).await.unwrap_err();
    assert!(
        err.as_error()
            .downcast_ref::<UnknownCommandPath>()
            .is_some()
    );
    let err = run(&warn(), b"{").await.unwrap_err();
    assert!(matches!(option_error(&err), OptionError::Malformed { .. }));
    let json = interaction(
        "ping",
        &json!([{ "name": "x", "type": 4, "value": 1 }]),
        None,
    );
    run(&ping(), &json).await.unwrap();
}
