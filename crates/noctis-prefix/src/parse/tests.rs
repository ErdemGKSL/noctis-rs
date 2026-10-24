//! Story 15.4 matrix rows R1-R37 over the pure functions.

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use std::any::TypeId;

use noctis_core::__rules::command::{Choice, ChoiceValue, NumericValue, OptionDecl, OptionType};
use noctis_core::{CommandInput, CommandMetadata, InteractionContext, OptionError, OptionValue};
use serde_json::{Value, json};
use twilight_model::channel::Message;
use twilight_model::id::Id;

use super::{Table, bind, is_ignored};
use crate::Opted;

const AUTHOR: u64 = 42;

static WARN: [OptionDecl<'static>; 3] = [
    OptionDecl::new("user", OptionType::User, "d").required(true),
    OptionDecl::new("days", OptionType::Integer, "d")
        .min_value(NumericValue::Int(1))
        .max_value(NumericValue::Int(30)),
    OptionDecl::new("reason", OptionType::String, "d").max_length(10),
];
static SAY: [OptionDecl<'static>; 1] =
    [OptionDecl::new("text", OptionType::String, "d").required(true)];
static CFG: [OptionDecl<'static>; 1] =
    [OptionDecl::new("value", OptionType::String, "d").required(true)];
static FLAG: [OptionDecl<'static>; 2] = [
    OptionDecl::new("on", OptionType::Boolean, "d").required(true),
    OptionDecl::new("ratio", OptionType::Number, "d"),
];
static COLORS: [Choice<'static>; 2] = [
    Choice::new("red", ChoiceValue::Str("red")),
    Choice::new("Green", ChoiceValue::Str("green")),
];
static PICK: [OptionDecl<'static>; 1] = [OptionDecl::new("color", OptionType::String, "d")
    .required(true)
    .choices(&COLORS)];
static FILE: [OptionDecl<'static>; 2] = [
    OptionDecl::new("doc", OptionType::Attachment, "d")
        .required(true)
        .file_types(&[".pdf"]),
    OptionDecl::new("note", OptionType::String, "d"),
];
static MEDIA: [OptionDecl<'static>; 1] =
    [OptionDecl::new("pic", OptionType::Attachment, "d").file_types(&["image"])];
static ROLE: [OptionDecl<'static>; 2] = [
    OptionDecl::new("r", OptionType::Role, "d").required(true),
    OptionDecl::new("m", OptionType::Mentionable, "d"),
];
static INT: [OptionDecl<'static>; 1] =
    [OptionDecl::new("n", OptionType::Integer, "d").required(true)];
static CHAN: [OptionDecl<'static>; 1] =
    [OptionDecl::new("c", OptionType::Channel, "d").required(true)];

static TWO: [OptionDecl<'static>; 2] = [
    OptionDecl::new("a", OptionType::String, "d"),
    OptionDecl::new("b", OptionType::String, "d"),
];

/// A message with `content`; `extra` overrides fields.
fn message_with(content: &str, extra: &Value) -> Message {
    let mut value = json!({
        "id": "100",
        "channel_id": "200",
        "guild_id": "300",
        "author": user(AUTHOR),
        "content": content,
        "timestamp": "2026-10-24T00:00:00.000000+00:00",
        "edited_timestamp": null,
        "tts": false,
        "mention_everyone": false,
        "mentions": [],
        "mention_roles": [],
        "attachments": [],
        "embeds": [],
        "pinned": false,
        "type": 0
    });
    for (key, v) in extra.as_object().unwrap() {
        value[key] = v.clone();
    }
    serde_json::from_value(value).unwrap()
}

fn message(content: &str) -> Message {
    message_with(content, &json!({}))
}

fn user(id: u64) -> Value {
    json!({
        "id": id.to_string(), "username": format!("u{id}"), "discriminator": "0",
        "avatar": null, "public_flags": 0
    })
}

fn attachment(id: u64, filename: &str, content_type: &str) -> Value {
    json!({
        "id": id.to_string(), "filename": filename, "content_type": content_type,
        "size": 1, "url": "https://cdn/x", "proxy_url": "https://media/x"
    })
}

fn input_of(args: &str, decls: &[OptionDecl<'_>], msg: &Message) -> CommandInput {
    bind("cmd", args, decls, msg)
}

fn input(args: &str, decls: &[OptionDecl<'_>]) -> CommandInput {
    input_of(args, decls, &message(""))
}

fn ok(args: &str, decls: &[OptionDecl<'_>]) -> CommandInput {
    let input = input(args, decls);
    assert_eq!(input.error(), None, "{args}");
    input
}

fn err(args: &str, decls: &[OptionDecl<'_>]) -> OptionError {
    input(args, decls).error().cloned().unwrap()
}

fn malformed(args: &str, decls: &[OptionDecl<'_>]) -> String {
    match err(args, decls) {
        OptionError::Malformed { reason } => reason,
        other => panic!("{args}: {other:?}"),
    }
}

fn constraint(args: &str, decls: &[OptionDecl<'_>]) -> (String, String) {
    match err(args, decls) {
        OptionError::Constraint { option, reason } => (option, reason),
        other => panic!("{args}: {other:?}"),
    }
}

fn wrong(option: &str, expected: OptionType, found: OptionType) -> OptionError {
    OptionError::WrongType {
        option: option.into(),
        expected,
        found,
    }
}

fn s(text: &str) -> OptionValue {
    OptionValue::String(text.into())
}

/// R1, R2.
#[test]
fn r1_r2_mentions() {
    let msg = message_with("", &json!({ "mentions": [user(5)] }));
    let input = input_of("<@5> 3 rude", &WARN, &msg);
    assert_eq!(input.error(), None);
    assert_eq!(input.get("user"), Some(&OptionValue::User(Id::new(5))));
    assert_eq!(input.get("days"), Some(&OptionValue::Integer(3)));
    assert_eq!(input.get("reason"), Some(&s("rude")));
    let resolved = &input.resolved().users[&Id::new(5)];
    assert_eq!(resolved.name, "u5");

    let input = input_of("<@!5>", &WARN, &msg);
    assert_eq!(input.get("user"), Some(&OptionValue::User(Id::new(5))));
    assert_eq!(input.get("days"), None);
    assert_eq!(input.get("reason"), None);
}

/// R3, R4.
#[test]
fn r3_r4_raw_ids() {
    let input = ok("5", &WARN);
    assert_eq!(input.get("user"), Some(&OptionValue::User(Id::new(5))));
    assert!(input.resolved().users.is_empty());
    let input = ok(&AUTHOR.to_string(), &WARN);
    assert_eq!(input.resolved().users[&Id::new(AUTHOR)].name, "u42");
}

/// R5-R9: rest-of-line STRING and quoting.
#[test]
fn r5_r9_rest_of_line_and_quotes() {
    assert_eq!(
        ok("hello  big   world", &SAY).get("text"),
        Some(&s("hello  big   world"))
    );
    assert_eq!(ok("\"hi there\"", &SAY).get("text"), Some(&s("hi there")));
    assert_eq!(
        ok("he said \"hi\"", &SAY).get("text"),
        Some(&s("he said \"hi\""))
    );
    assert_eq!(ok("\"a\" b", &SAY).get("text"), Some(&s("\"a\" b")));
    assert_eq!(ok("5 3 \"a b\"", &WARN).get("reason"), Some(&s("a b")));
    assert_eq!(
        ok(r#""a \"q\" \\ b""#, &SAY).get("text"),
        Some(&s(r#"a "q" \ b"#))
    );
    assert_eq!(ok(r#""a\nb""#, &SAY).get("text"), Some(&s(r"a\nb")));
    // A quoted token mid-line, before the rest-of-line option.
    let input = ok("\"x y\" z w", &TWO);
    assert_eq!(input.get("a"), Some(&s("x y")));
    assert_eq!(input.get("b"), Some(&s("z w")));
}

/// R10-R12: grammar errors.
#[test]
fn r10_r12_grammar_errors() {
    assert!(malformed("5 \"3", &WARN).contains("unclosed quote"));
    assert!(malformed("\"true\"x", &FLAG).contains("after closing quote"));
    assert!(malformed("true 1.5 extra", &FLAG).contains("unexpected argument `extra`"));
}

/// R13: a missing required option.
#[test]
fn r13_missing() {
    assert_eq!(
        err("", &WARN),
        OptionError::Missing {
            option: "user".into()
        }
    );
}

/// R14-R20: INTEGER.
#[test]
fn r14_r20_integers() {
    assert_eq!(
        err("5 abc", &WARN),
        wrong("days", OptionType::Integer, OptionType::String)
    );
    assert_eq!(
        err("5 1.5", &WARN),
        wrong("days", OptionType::Integer, OptionType::Number)
    );
    assert_eq!(
        err("5 99999999999999999999", &WARN),
        OptionError::OutOfRange {
            option: "days".into(),
            value: "99999999999999999999".into()
        }
    );
    let (option, reason) = constraint("9007199254740992", &INT);
    assert_eq!(option, "n");
    assert!(reason.starts_with("[OPT-03]"), "{reason}");
    assert_eq!(
        ok("9007199254740991", &INT).get("n"),
        Some(&OptionValue::Integer(9_007_199_254_740_991))
    );
    for low_high in ["5 0", "5 31"] {
        let (option, reason) = constraint(low_high, &WARN);
        assert_eq!(option, "days");
        assert!(reason.starts_with("[OPT-05]"), "{reason}");
    }
    assert_eq!(ok("5 1", &WARN).get("days"), Some(&OptionValue::Integer(1)));
    assert_eq!(
        ok("5 30", &WARN).get("days"),
        Some(&OptionValue::Integer(30))
    );
    assert_eq!(ok("+5", &INT).get("n"), Some(&OptionValue::Integer(5)));
    assert_eq!(ok("-0", &INT).get("n"), Some(&OptionValue::Integer(0)));
    for token in ["١٢", "0x10", "1_000", "-", "+"] {
        assert_eq!(
            err(token, &INT),
            wrong("n", OptionType::Integer, OptionType::String),
            "{token}"
        );
    }
}

/// R21, R22: NUMBER.
#[test]
fn r21_r22_numbers() {
    for (token, value) in [("1e3", 1000.0), (".5", 0.5), ("-2.", -2.0), ("+1E-1", 0.1)] {
        assert_eq!(
            ok(&format!("true {token}"), &FLAG).get("ratio"),
            Some(&OptionValue::Number(value)),
            "{token}"
        );
    }
    for token in ["nan", "inf", "1e", ".", "e5"] {
        assert_eq!(
            err(&format!("true {token}"), &FLAG),
            wrong("ratio", OptionType::Number, OptionType::String),
            "{token}"
        );
    }
    for token in ["1e400", "9007199254740994"] {
        let (option, reason) = constraint(&format!("true {token}"), &FLAG);
        assert_eq!(option, "ratio");
        assert!(reason.starts_with("[OPT-04]"), "{reason}");
    }
}

/// R23: BOOLEAN.
#[test]
fn r23_booleans() {
    for (token, value) in [
        ("true", true),
        ("YES", true),
        ("On", true),
        ("false", false),
        ("No", false),
        ("OFF", false),
    ] {
        assert_eq!(
            ok(token, &FLAG).get("on"),
            Some(&OptionValue::Boolean(value))
        );
    }
    for token in ["1", "maybe"] {
        assert_eq!(
            err(token, &FLAG),
            wrong("on", OptionType::Boolean, OptionType::String)
        );
    }
}

/// R24: STRING length counts `discord_len`.
#[test]
fn r24_string_length() {
    let (option, reason) = constraint("5 3 12345678901", &WARN);
    assert_eq!(option, "reason");
    assert!(reason.starts_with("[OPT-06]"), "{reason}");
    assert_eq!(
        ok("5 3 1234567890", &WARN).get("reason"),
        Some(&s("1234567890"))
    );
    // 🙂 is two UTF-16 units: five fit max_length 10, six do not.
    assert!(
        ok(&format!("5 3 {}", "🙂".repeat(5)), &WARN)
            .error()
            .is_none()
    );
    let (_, reason) = constraint(&format!("5 3 {}", "🙂".repeat(10)), &WARN);
    assert!(reason.starts_with("[OPT-06] length 20"), "{reason}");
}

/// R25, R26: choices.
#[test]
fn r25_r26_choices() {
    assert_eq!(ok("red", &PICK).get("color"), Some(&s("red")));
    assert_eq!(ok("Green", &PICK).get("color"), Some(&s("green")));
    assert_eq!(ok("GREEN", &PICK).get("color"), Some(&s("green")));
    assert_eq!(
        err("blue", &PICK),
        OptionError::UnknownChoice {
            option: "color".into(),
            value: "blue".into()
        }
    );
}

/// Numeric choices bind by name or by value.
#[test]
fn numeric_choices() {
    static LEVELS: [Choice<'static>; 2] = [
        Choice::new("Low", ChoiceValue::Int(1)),
        Choice::new("High", ChoiceValue::Int(9)),
    ];
    static LEVEL: [OptionDecl<'static>; 1] =
        [OptionDecl::new("level", OptionType::Integer, "d").choices(&LEVELS)];
    assert_eq!(
        ok("high", &LEVEL).get("level"),
        Some(&OptionValue::Integer(9))
    );
    assert_eq!(ok("1", &LEVEL).get("level"), Some(&OptionValue::Integer(1)));
    assert_eq!(
        err("2", &LEVEL),
        OptionError::UnknownChoice {
            option: "level".into(),
            value: "2".into()
        }
    );
}

/// R27-R29: attachments.
#[test]
fn r27_r29_attachments() {
    let msg = message_with(
        "",
        &json!({ "attachments": [attachment(7, "a.PDF", "application/pdf")] }),
    );
    let input = input_of("see this", &FILE, &msg);
    assert_eq!(input.error(), None);
    assert_eq!(input.get("doc"), Some(&OptionValue::Attachment(Id::new(7))));
    assert!(input.resolved().attachments.contains_key(&Id::new(7)));
    assert_eq!(input.get("note"), Some(&s("see this")));

    let png = message_with(
        "",
        &json!({ "attachments": [attachment(8, "a.png", "image/png")] }),
    );
    match input_of("", &FILE, &png).error() {
        Some(OptionError::Constraint { option, reason }) => {
            assert_eq!(option, "doc");
            assert!(reason.starts_with("[OPT-13]"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    let input = input_of("", &MEDIA, &png);
    assert_eq!(input.get("pic"), Some(&OptionValue::Attachment(Id::new(8))));
    assert!(input_of("", &MEDIA, &msg).error().is_some());

    assert_eq!(
        err("just text", &FILE),
        OptionError::Missing {
            option: "doc".into()
        }
    );
}

/// R30, R31: roles, mentionables and bad ids.
#[test]
fn r30_r31_ids() {
    let input = ok("<@&7> <@&8>", &ROLE);
    assert_eq!(input.get("r"), Some(&OptionValue::Role(Id::new(7))));
    assert_eq!(input.get("m"), Some(&OptionValue::Mentionable(Id::new(8))));
    let msg = message_with("", &json!({ "mentions": [user(9)] }));
    let input = input_of("7 <@9>", &ROLE, &msg);
    assert_eq!(input.get("r"), Some(&OptionValue::Role(Id::new(7))));
    assert_eq!(input.get("m"), Some(&OptionValue::Mentionable(Id::new(9))));
    assert!(input.resolved().users.contains_key(&Id::new(9)));
    assert_eq!(
        ok("<#11>", &CHAN).get("c"),
        Some(&OptionValue::Channel(Id::new(11)))
    );

    for token in [
        "<@0>",
        "<@abc>",
        "@everyone",
        "123456789012345678901",
        "<@&7>",
        "<#7>",
    ] {
        assert_eq!(
            err(token, &WARN),
            wrong("user", OptionType::User, OptionType::String),
            "{token}"
        );
    }
    assert_eq!(
        err("<@7>", &ROLE),
        wrong("r", OptionType::Role, OptionType::String)
    );
}

/// R32: a unit command takes no arguments.
#[test]
fn r32_unit_command() {
    assert!(malformed("x", &[]).contains("unexpected argument `x`"));
    assert_eq!(ok("", &[]).error(), None);
}

/// The first failure in declaration order wins.
#[test]
fn first_error_wins() {
    assert_eq!(
        err("abc 0 12345678901", &WARN),
        wrong("user", OptionType::User, OptionType::String)
    );
}

struct Warn;
struct Cfg;
struct Unlu;

fn opted<T: 'static>(
    paths: &'static [&'static str],
    options: &'static [OptionDecl<'static>],
) -> Opted {
    Opted {
        type_id: TypeId::of::<T>(),
        name: "bot::T",
        paths,
        options,
        ephemerality: noctis_core::Ephemerality::Public,
        metadata: CommandMetadata::new(),
    }
}

fn table(prefix: &str) -> Table {
    Table::new(
        prefix,
        &[
            opted::<Warn>(&["warn"], &WARN),
            opted::<Cfg>(&["config set prefix"], &CFG),
            opted::<Unlu>(&["ünlü"], &[]),
        ],
    )
}

fn found<'c>(table: &Table, content: &'c str) -> Option<(&'static str, &'c str)> {
    table
        .find(content)
        .map(|(entry, args)| (entry.path(), args))
}

/// R33-R37: paths and prefixes.
#[test]
fn r33_r37_paths() {
    let t = table("!");
    assert_eq!(
        found(&t, "!config  set\u{3000}prefix ?"),
        Some(("config set prefix", " ?"))
    );
    assert_eq!(found(&t, "!WARN <@5>"), Some(("warn", " <@5>")));
    assert_eq!(found(&t, "!ÜNLÜ"), Some(("ünlü", "")));
    for ignored in [
        "!config set",
        "!warn5",
        "!config",
        "!",
        "! warn 5",
        "?warn 5",
        "!!warn",
        "",
        "warn",
    ] {
        assert_eq!(found(&t, ignored), None, "{ignored}");
    }
    let t = table("ş>");
    assert_eq!(found(&t, "ş>warn <@5>"), Some(("warn", " <@5>")));
    assert_eq!(found(&t, "Ş>warn"), None);
}

/// The final sigma lowercases by position, like `str::to_lowercase`.
#[test]
fn sigma_lowercase() {
    assert!(super::lowercase_eq("ΟΔΟΣ", "\u{3bf}\u{3b4}\u{3bf}\u{3c2}"));
    assert!(!super::lowercase_eq("ΟΔΟΣ", "\u{3bf}\u{3b4}\u{3bf}\u{3c3}"));
    assert!(super::lowercase_eq("WaRn", "warn"));
}

/// R38: bot, webhook and system messages are ignored.
#[test]
fn r38_ignored_messages() {
    assert!(!is_ignored(&message("!warn")));
    assert!(!is_ignored(&message_with("!warn", &json!({ "type": 19 }))));
    let mut bot = user(1);
    bot["bot"] = json!(true);
    assert!(is_ignored(&message_with(
        "!warn",
        &json!({ "author": bot })
    )));
    assert!(is_ignored(&message_with(
        "!warn",
        &json!({ "webhook_id": "9" })
    )));
    assert!(is_ignored(&message_with("!warn", &json!({ "type": 7 }))));
}

/// R39: contexts.
#[test]
fn r39_contexts() {
    let mut guild_only = opted::<Warn>(&["warn"], &WARN);
    guild_only.metadata = CommandMetadata::new().contexts(&[InteractionContext::Guild]);
    let mut dm_only = opted::<Cfg>(&["dm"], &[]);
    dm_only.metadata = CommandMetadata::new().contexts(&[InteractionContext::BotDm]);
    let t = Table::new("!", &[guild_only, dm_only, opted::<Unlu>(&["any"], &[])]);
    let allowed = |content: &str, in_guild: bool| t.find(content).unwrap().0.allowed(in_guild);
    assert!(allowed("!warn", true));
    assert!(!allowed("!warn", false));
    assert!(allowed("!dm", false));
    assert!(!allowed("!dm", true));
    assert!(allowed("!any", true) && allowed("!any", false));
}
