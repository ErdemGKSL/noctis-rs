//! Command, context-menu and autocomplete `data`, checked against the
//! command definitions the App publishes.

use serde_json::{Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, MessageMarker, UserMarker};

use super::wire::{self, Resolved};
use crate::TestError;
use crate::response::RunCtx;
use crate::seed::{Seeds, TestOption};

/// The first id of a simulated command (`data.id`).
const FIRST_COMMAND_ID: u64 = 1_150_000_000_000_000_000;
/// The largest integer Discord accepts (2^53 - 1).
const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// The command definitions of an App, with their scopes.
pub(crate) type Definitions = [(Option<Id<GuildMarker>>, Value)];

/// The Discord type name of option type `kind`.
const fn type_name(kind: u64) -> &'static str {
    match kind {
        1 => "SUB_COMMAND",
        2 => "SUB_COMMAND_GROUP",
        3 => "STRING",
        4 => "INTEGER",
        5 => "BOOLEAN",
        6 => "USER",
        7 => "CHANNEL",
        8 => "ROLE",
        9 => "MENTIONABLE",
        10 => "NUMBER",
        11 => "ATTACHMENT",
        _ => "UNKNOWN",
    }
}

fn kind(value: &Value) -> u64 {
    value.get("type").and_then(Value::as_u64).unwrap_or(1)
}

fn name(value: &Value) -> &str {
    value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn options(value: &Value) -> &[Value] {
    value
        .get("options")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// The definition of command `base` of `kind` visible in `guild`: a
/// guild-scoped one first, then a global one; with its `data.id` and
/// scope.
fn find<'a>(
    definitions: &'a Definitions,
    base: &str,
    command_kind: u64,
    guild: Option<Id<GuildMarker>>,
) -> Option<(u64, Option<Id<GuildMarker>>, &'a Value)> {
    let visible = |scope: Option<Id<GuildMarker>>| scope.is_none() || scope == guild;
    let mut found = None;
    for (index, (scope, command)) in definitions.iter().enumerate() {
        if kind(command) != command_kind || name(command) != base || !visible(*scope) {
            continue;
        }
        let id = FIRST_COMMAND_ID + u64::try_from(index).unwrap_or(0);
        if scope.is_some() {
            return Some((id, *scope, command));
        }
        found.get_or_insert((id, *scope, command));
    }
    found
}

/// What a chat-input run sends.
pub(crate) struct ChatInput<'a> {
    pub(crate) path: &'a str,
    pub(crate) options: &'a [(String, TestOption)],
    /// `(option, typed)` of an autocomplete run.
    pub(crate) focus: Option<(&'a str, &'a str)>,
}

/// The `data` of a chat-input (or autocomplete) run.
pub(crate) fn chat_input(
    definitions: &Definitions,
    run: &ChatInput<'_>,
    seeds: &Seeds,
    ctx: &RunCtx,
) -> Result<Value, TestError> {
    let unknown = || TestError::UnknownCommand {
        path: run.path.to_owned(),
    };
    let mut words = run.path.split_whitespace();
    let base = words.next().ok_or_else(unknown)?;
    let (id, scope, command) = find(definitions, base, 1, ctx.guild).ok_or_else(unknown)?;
    let mut node = command;
    let mut nesting: Vec<(u64, &str)> = Vec::new();
    for word in words {
        let child = options(node)
            .iter()
            .find(|o| matches!(kind(o), 1 | 2) && name(o) == word)
            .ok_or_else(unknown)?;
        nesting.push((kind(child), word));
        node = child;
    }
    let declared = options(node);
    if declared.iter().any(|o| matches!(kind(o), 1 | 2)) {
        return Err(unknown());
    }
    let mut resolved = Resolved::default();
    let mut values: Vec<(String, Value)> = Vec::new();
    for (option, value) in run.options {
        let decl = declared.iter().find(|d| name(d) == option).ok_or_else(|| {
            TestError::UnknownOption {
                path: run.path.to_owned(),
                option: option.clone(),
            }
        })?;
        let json = option_value(option, decl, value, seeds, ctx, &mut resolved)?;
        values.retain(|(n, _)| n != option);
        values.push((option.clone(), json));
    }
    let mut sent: Vec<Value> = values
        .iter()
        .filter_map(|(option, value)| {
            let decl = declared.iter().find(|d| name(d) == option)?;
            Some(json!({ "type": kind(decl), "name": option, "value": value }))
        })
        .collect();
    match run.focus {
        Some((option, typed)) => {
            let decl = declared.iter().find(|d| name(d) == option).ok_or_else(|| {
                TestError::UnknownOption {
                    path: run.path.to_owned(),
                    option: option.to_owned(),
                }
            })?;
            if decl.get("autocomplete").and_then(Value::as_bool) != Some(true) {
                return Err(TestError::NotAutocomplete {
                    option: option.to_owned(),
                });
            }
            sent.retain(|o| name(o) != option);
            sent.push(
                json!({ "type": kind(decl), "name": option, "value": typed, "focused": true }),
            );
        }
        None => {
            if let Some(missing) = declared.iter().find(|d| {
                d.get("required").and_then(Value::as_bool) == Some(true)
                    && !values.iter().any(|(n, _)| n == name(d))
            }) {
                return Err(TestError::MissingOption {
                    option: name(missing).to_owned(),
                });
            }
        }
    }
    for (kind, word) in nesting.into_iter().rev() {
        sent = vec![json!({ "type": kind, "name": word, "options": sent })];
    }
    let mut data = json!({ "id": id.to_string(), "name": base, "type": 1, "options": sent });
    finish(&mut data, scope, resolved);
    Ok(data)
}

/// The target of a context-menu run.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Target {
    pub(crate) user: Option<Id<UserMarker>>,
    pub(crate) message: Option<Id<MessageMarker>>,
}

/// The `data` of a USER (`menu_kind` 2) or MESSAGE (3) command run.
pub(crate) fn context_menu(
    definitions: &Definitions,
    menu: &str,
    menu_kind: u64,
    target: Target,
    seeds: &Seeds,
    ctx: &RunCtx,
) -> Result<Value, TestError> {
    let (id, scope, _) =
        find(definitions, menu, menu_kind, ctx.guild).ok_or_else(|| TestError::UnknownCommand {
            path: menu.to_owned(),
        })?;
    let mut resolved = Resolved::default();
    let target_id = match (menu_kind, target.user, target.message) {
        (2, Some(user), None) => {
            resolved.user(seeds, ctx, user)?;
            user.to_string()
        }
        (3, None, Some(message)) => {
            let json = wire::message_json(message, ctx.channel, &json!({ "content": "" }));
            resolved.message(message, json);
            message.to_string()
        }
        (2 | 3, None, None) => {
            return Err(TestError::MissingOption {
                option: if menu_kind == 2 {
                    "target_user"
                } else {
                    "target_message"
                }
                .to_owned(),
            });
        }
        _ => {
            return Err(TestError::InvalidOption {
                option: "target".to_owned(),
                reason: "a user command takes .target_user(..), a message command .target_message(..)",
            });
        }
    };
    let mut data =
        json!({ "id": id.to_string(), "name": menu, "type": menu_kind, "target_id": target_id });
    finish(&mut data, scope, resolved);
    Ok(data)
}

/// Adds `guild_id` (guild-scoped commands) and `resolved` to `data`.
fn finish(data: &mut Value, scope: Option<Id<GuildMarker>>, resolved: Resolved) {
    if let Some(guild) = scope {
        data["guild_id"] = json!(guild.to_string());
    }
    if let Some(resolved) = resolved.into_value() {
        data["resolved"] = resolved;
    }
}

/// The JSON value of `value` for option declaration `decl`, checked like
/// Discord's client: type, safe integers, finite numbers and `choices`.
/// Length and range bounds are not checked, so the App's validators run.
fn option_value(
    option: &str,
    decl: &Value,
    value: &TestOption,
    seeds: &Seeds,
    ctx: &RunCtx,
    resolved: &mut Resolved,
) -> Result<Value, TestError> {
    let declared = kind(decl);
    let mismatch = || TestError::OptionType {
        option: option.to_owned(),
        expected: type_name(declared),
        found: value.type_name(),
    };
    let invalid = |reason| TestError::InvalidOption {
        option: option.to_owned(),
        reason,
    };
    let json = match (declared, value) {
        (3, TestOption::String(s)) => json!(s),
        (4 | 10, TestOption::Integer(i)) => {
            if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(i) {
                return Err(invalid("integers must be within ±(2^53 - 1)"));
            }
            json!(i)
        }
        (10, TestOption::Number(f)) => {
            if !f.is_finite() {
                return Err(invalid("numbers must be finite"));
            }
            json!(f)
        }
        (5, TestOption::Boolean(b)) => json!(b),
        (6 | 9, TestOption::User(id)) => {
            resolved.user(seeds, ctx, *id)?;
            json!(id.to_string())
        }
        (7, TestOption::Channel(id)) => {
            resolved.channel(seeds, ctx, *id)?;
            json!(id.to_string())
        }
        (8 | 9, TestOption::Role(id)) => {
            resolved.role(seeds, ctx, *id)?;
            json!(id.to_string())
        }
        (11, _) => {
            return Err(TestError::Unsupported(
                "attachment options [planned, Story 9.1]",
            ));
        }
        _ => return Err(mismatch()),
    };
    let choices = options_of(decl, "choices");
    if !choices.is_empty()
        && !choices
            .iter()
            .any(|c| c.get("value") == Some(&json) || same_number(c.get("value"), &json))
    {
        return Err(TestError::NotAChoice {
            option: option.to_owned(),
            value: value.display(),
        });
    }
    Ok(json)
}

fn options_of<'a>(decl: &'a Value, key: &str) -> &'a [Value] {
    decl.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Whether two JSON numbers are equal as `f64` (`5` and `5.0`).
fn same_number(a: Option<&Value>, b: &Value) -> bool {
    match (a.and_then(Value::as_f64), b.as_f64()) {
        (Some(a), Some(b)) => a.to_bits() == b.to_bits(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use twilight_model::guild::Permissions;

    use super::*;
    use crate::seed::{TestGuild, TestUser};

    fn definitions() -> Vec<(Option<Id<GuildMarker>>, Value)> {
        vec![
            (
                None,
                json!({ "type": 1, "name": "ping", "description": "d", "options": [] }),
            ),
            (
                None,
                json!({ "type": 1, "name": "setup", "description": "d", "options": [
                    { "type": 1, "name": "start", "description": "d", "options": [] },
                ]}),
            ),
            (
                None,
                json!({ "type": 1, "name": "config", "description": "d", "options": [
                    { "type": 2, "name": "config", "description": "d", "options": [
                        { "type": 1, "name": "set", "description": "d", "options": [
                            { "type": 3, "name": "prefix", "description": "d" },
                        ]},
                    ]},
                ]}),
            ),
            (
                None,
                json!({ "type": 1, "name": "opts", "description": "d", "options": [
                    { "type": 4, "name": "count", "description": "d", "required": true },
                    { "type": 10, "name": "ratio", "description": "d" },
                    { "type": 3, "name": "mode", "description": "d", "choices": [
                        { "name": "Fast", "value": "fast" }, { "name": "Slow", "value": "slow" },
                    ]},
                    { "type": 6, "name": "user", "description": "d" },
                    { "type": 7, "name": "channel", "description": "d" },
                    { "type": 8, "name": "role", "description": "d" },
                    { "type": 9, "name": "who", "description": "d" },
                    { "type": 3, "name": "rule", "description": "d", "autocomplete": true },
                ]}),
            ),
            (
                Some(Id::new(10)),
                json!({ "type": 1, "name": "local", "description": "d", "options": [] }),
            ),
        ]
    }

    fn seeds() -> Seeds {
        let mut seeds = Seeds::default();
        for user in [
            TestUser::new(Id::new(40)).permissions(Permissions::MANAGE_GUILD),
            TestUser::new(Id::new(41)),
        ] {
            seeds.users.insert(user.id, user);
        }
        let g = TestGuild::new(Id::new(10), "Guild")
            .text_channel(Id::new(20), "logs")
            .role(Id::new(30), "Mods");
        seeds.guilds.insert(g.id, g);
        seeds
    }

    fn ctx(guild: Option<u64>) -> RunCtx {
        RunCtx {
            user: Id::new(40),
            guild: guild.map(Id::new),
            channel: Id::new(20),
            channel_type: 0,
            locale: "en-US".to_owned(),
            guild_locale: None,
        }
    }

    fn run(path: &str, options: &[(&str, TestOption)]) -> Result<Value, TestError> {
        let options: Vec<(String, TestOption)> = options
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect();
        chat_input(
            &definitions(),
            &ChatInput {
                path,
                options: &options,
                focus: None,
            },
            &seeds(),
            &ctx(Some(10)),
        )
    }

    /// C1, C2: one word, a subcommand, a group.
    #[test]
    fn paths_nest_their_options() {
        let ping = run("ping", &[]).unwrap();
        assert_eq!(
            (ping["name"].clone(), ping["type"].clone()),
            (json!("ping"), json!(1))
        );
        let start = run("setup start", &[]).unwrap();
        assert_eq!(
            start["options"],
            json!([{ "type": 1, "name": "start", "options": [] }])
        );
        let set = run("config config set", &[("prefix", "!".into())]).unwrap();
        assert_eq!(set["options"][0]["type"], 2);
        assert_eq!(set["options"][0]["options"][0]["type"], 1);
        assert_eq!(
            set["options"][0]["options"][0]["options"][0],
            json!({ "type": 3, "name": "prefix", "value": "!" })
        );
    }

    /// C3: unknown words, incomplete paths and other guilds' commands.
    #[test]
    fn unknown_paths() {
        for path in ["setp start", "setup", "setup start more", ""] {
            assert!(
                matches!(run(path, &[]), Err(TestError::UnknownCommand { .. })),
                "{path}"
            );
        }
        assert!(run("local", &[]).is_ok());
        let other = chat_input(
            &definitions(),
            &ChatInput {
                path: "local",
                options: &[],
                focus: None,
            },
            &seeds(),
            &ctx(None),
        );
        assert!(matches!(other, Err(TestError::UnknownCommand { .. })));
        assert_eq!(run("local", &[]).unwrap()["guild_id"], "10");
    }

    /// C5-C10: types, safe integers, finite numbers, unknown, missing and
    /// choice options.
    #[test]
    fn option_checks() {
        let ok = run("opts", &[("count", 5.into()), ("ratio", 5.into())]).unwrap();
        assert_eq!(
            ok["options"][0],
            json!({ "type": 4, "name": "count", "value": 5 })
        );
        assert_eq!(
            ok["options"][1],
            json!({ "type": 10, "name": "ratio", "value": 5 })
        );
        let half = run("opts", &[("count", 1.into()), ("ratio", 0.5.into())]).unwrap();
        assert_eq!(half["options"][1]["value"], 0.5);
        assert!(matches!(
            run("opts", &[("count", 0.5.into())]),
            Err(TestError::OptionType {
                expected: "INTEGER",
                found: "NUMBER",
                ..
            })
        ));
        assert!(matches!(
            run("opts", &[("count", "x".into())]),
            Err(TestError::OptionType {
                expected: "INTEGER",
                found: "STRING",
                ..
            })
        ));
        assert!(matches!(
            run("opts", &[("count", 1.into()), ("ratio", f64::NAN.into())]),
            Err(TestError::InvalidOption { .. })
        ));
        assert!(matches!(
            run("opts", &[("count", 2_i64.pow(53).into())]),
            Err(TestError::InvalidOption { .. })
        ));
        assert!(run("opts", &[("count", (2_i64.pow(53) - 1).into())]).is_ok());
        assert!(matches!(
            run("opts", &[("count", 1.into()), ("nope", 1.into())]),
            Err(TestError::UnknownOption { option, .. }) if option == "nope"
        ));
        assert!(matches!(
            run("opts", &[]),
            Err(TestError::MissingOption { option }) if option == "count"
        ));
        assert!(matches!(
            run("opts", &[("count", 1.into()), ("mode", "medium".into())]),
            Err(TestError::NotAChoice { value, .. }) if value == "medium"
        ));
        assert!(run("opts", &[("count", 1.into()), ("mode", "fast".into())]).is_ok());
    }

    /// C11, C12: entity options carry `resolved` data from the seeds.
    #[test]
    fn entity_options_resolve() {
        let d = run(
            "opts",
            &[
                ("count", 1.into()),
                ("user", Id::<UserMarker>::new(41).into()),
                (
                    "channel",
                    Id::<twilight_model::id::marker::ChannelMarker>::new(20).into(),
                ),
                (
                    "role",
                    Id::<twilight_model::id::marker::RoleMarker>::new(30).into(),
                ),
                ("who", Id::<UserMarker>::new(40).into()),
            ],
        )
        .unwrap();
        assert_eq!(
            d["options"][1],
            json!({ "type": 6, "name": "user", "value": "41" })
        );
        let resolved = &d["resolved"];
        assert_eq!(resolved["users"]["41"]["id"], "41");
        assert_eq!(resolved["members"]["41"]["roles"], json!([]));
        assert_eq!(resolved["channels"]["20"]["type"], 0);
        assert_eq!(resolved["roles"]["30"]["name"], "Mods");
        assert_eq!(resolved["users"]["40"]["id"], "40");
        assert!(matches!(
            run(
                "opts",
                &[
                    ("count", 1.into()),
                    ("user", Id::<UserMarker>::new(99).into())
                ]
            ),
            Err(TestError::Unknown {
                kind: "user",
                id: 99
            })
        ));
    }

    /// A3: a focused option needs the `autocomplete` flag.
    #[test]
    fn autocomplete_focus() {
        let focus = |option| {
            chat_input(
                &definitions(),
                &ChatInput {
                    path: "opts",
                    options: &[],
                    focus: Some((option, "sp")),
                },
                &seeds(),
                &ctx(Some(10)),
            )
        };
        let d = focus("rule").unwrap();
        assert_eq!(
            d["options"],
            json!([{ "type": 3, "name": "rule", "value": "sp", "focused": true }])
        );
        assert!(matches!(
            focus("mode"),
            Err(TestError::NotAutocomplete { .. })
        ));
        assert!(matches!(
            focus("nope"),
            Err(TestError::UnknownOption { .. })
        ));
    }
}
