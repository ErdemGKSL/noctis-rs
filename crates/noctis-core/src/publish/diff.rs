//! The canonical command form and the diff (FR-13, REG-05): Discord fills
//! in defaults and server fields on every command it returns, so both
//! sides are reduced to the fields that matter, with Discord's documented
//! defaults applied, before they are compared.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::kind_name;

/// A command's identity: `(type, name)`.
pub(super) type Key = (u64, String);

/// The defaults a canonical form fills in.
pub(super) struct Defaults {
    /// The application's enabled installation types (sorted): an unset
    /// `integration_types` means these.
    pub(super) integration_types: Vec<u64>,
    /// A guild target: `contexts` and `integration_types` do not apply
    /// (META-03) and are left out of the comparison.
    pub(super) guild: bool,
}

/// Discord's default `contexts` for a new command.
const DEFAULT_CONTEXTS: [u64; 3] = [0, 1, 2];

/// The changes between a local and a remote command list.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Diff {
    /// One line per change, sorted: creates, updates, deletes; then type
    /// and name.
    pub(super) lines: Vec<String>,
    /// The `+` lines.
    pub(super) creates: usize,
}

/// The canonical form of a chat-input, user or message command, keyed by
/// `(type, name)`; `None` for anything else (no object, no name, another
/// type).
pub(super) fn canonical(command: &Value, defaults: &Defaults) -> Option<(Key, Value)> {
    let object = command.as_object()?;
    let kind = match object.get("type") {
        None | Some(Value::Null) => 1,
        Some(value) => value.as_u64()?,
    };
    if !(1..=3).contains(&kind) {
        return None;
    }
    let name = object.get("name")?.as_str()?.to_owned();
    let mut out = Map::new();
    out.insert("type".into(), Value::from(kind));
    out.insert("name".into(), Value::from(name.clone()));
    out.insert("description".into(), text(object.get("description")));
    out.insert("options".into(), options(object.get("options")));
    let permissions = match object.get("default_member_permissions") {
        Some(Value::String(bits)) => Value::from(bits.clone()),
        Some(Value::Number(bits)) => Value::from(bits.to_string()),
        _ => Value::Null,
    };
    out.insert("default_member_permissions".into(), permissions);
    out.insert("nsfw".into(), flag(object.get("nsfw")));
    if !defaults.guild {
        out.insert(
            "contexts".into(),
            set(object.get("contexts"), &DEFAULT_CONTEXTS),
        );
        out.insert(
            "integration_types".into(),
            set(object.get("integration_types"), &defaults.integration_types),
        );
    }
    localizations(object, &mut out);
    Some(((kind, name), Value::Object(out)))
}

/// A string, with absent and `null` counting as `""`.
fn text(value: Option<&Value>) -> Value {
    match value {
        None | Some(Value::Null) => Value::from(""),
        Some(value) => value.clone(),
    }
}

/// A boolean, with absent and `null` counting as `false`.
fn flag(value: Option<&Value>) -> Value {
    match value {
        None | Some(Value::Null) => Value::Bool(false),
        Some(value) => value.clone(),
    }
}

/// A list, with absent and `null` counting as `[]`.
fn list(value: Option<&Value>) -> Value {
    match value {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(value) => value.clone(),
    }
}

/// A number compared as `f64` (`1` equals `1.0`), absent as `null`.
fn number(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_f64)
        .map_or(Value::Null, Value::from)
}

/// An integer list compared as a set; absent and `null` mean `default`.
fn set(value: Option<&Value>, default: &[u64]) -> Value {
    let items: BTreeSet<u64> = match value {
        None | Some(Value::Null) => default.iter().copied().collect(),
        Some(Value::Array(items)) => {
            let numbers: Option<BTreeSet<u64>> = items.iter().map(Value::as_u64).collect();
            match numbers {
                Some(numbers) => numbers,
                None => return value.cloned().unwrap_or(Value::Null),
            }
        }
        Some(other) => return other.clone(),
    };
    Value::Array(items.into_iter().map(Value::from).collect())
}

/// `name_localizations` and `description_localizations`, with absent,
/// `null` and `{}` equal.
fn localizations(object: &Map<String, Value>, out: &mut Map<String, Value>) {
    for key in ["name_localizations", "description_localizations"] {
        let value = match object.get(key) {
            Some(Value::Object(map)) if !map.is_empty() => Value::Object(map.clone()),
            None | Some(Value::Null | Value::Object(_)) => Value::Object(Map::new()),
            Some(other) => other.clone(),
        };
        out.insert(key.into(), value);
    }
}

/// An `options` array in canonical form (order kept).
fn options(value: Option<&Value>) -> Value {
    match value {
        None | Some(Value::Null) => Value::Array(Vec::new()),
        Some(Value::Array(items)) => Value::Array(items.iter().map(option).collect()),
        Some(other) => other.clone(),
    }
}

/// One option (or subcommand, or group) in canonical form.
fn option(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut out = Map::new();
    out.insert(
        "type".into(),
        object.get("type").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".into(),
        object.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert("description".into(), text(object.get("description")));
    out.insert("required".into(), flag(object.get("required")));
    out.insert("autocomplete".into(), flag(object.get("autocomplete")));
    let choices = match object.get("choices") {
        Some(Value::Array(items)) => Value::Array(items.iter().map(choice).collect()),
        other => list(other),
    };
    out.insert("choices".into(), choices);
    out.insert("channel_types".into(), list(object.get("channel_types")));
    out.insert("file_types".into(), list(object.get("file_types")));
    out.insert("min_value".into(), number(object.get("min_value")));
    out.insert("max_value".into(), number(object.get("max_value")));
    for key in ["min_length", "max_length"] {
        out.insert(key.into(), object.get(key).cloned().unwrap_or(Value::Null));
    }
    out.insert("options".into(), options(object.get("options")));
    localizations(object, &mut out);
    Value::Object(out)
}

/// One choice: its name, its value (numbers compared as `f64`) and its
/// name localizations.
fn choice(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut out = Map::new();
    out.insert(
        "name".into(),
        object.get("name").cloned().unwrap_or(Value::Null),
    );
    let choice_value = match object.get("value") {
        Some(Value::Number(n)) => n.as_f64().map_or(Value::Null, Value::from),
        Some(other) => other.clone(),
        None => Value::Null,
    };
    out.insert("value".into(), choice_value);
    let names = match object.get("name_localizations") {
        Some(Value::Object(map)) if !map.is_empty() => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    };
    out.insert("name_localizations".into(), names);
    Value::Object(out)
}

/// The canonical keys whose values differ, sorted.
fn changed_keys(local: &Value, remote: &Value) -> Vec<String> {
    let (Some(local), Some(remote)) = (local.as_object(), remote.as_object()) else {
        return vec!["command".to_owned()];
    };
    let keys: BTreeSet<&String> = local.keys().chain(remote.keys()).collect();
    keys.into_iter()
        .filter(|key| local.get(*key) != remote.get(*key))
        .cloned()
        .collect()
}

/// The diff from `remote` to `local`; commands match by `(type, name)`,
/// command order does not matter.
pub(super) fn diff(local: &[(Key, Value)], remote: &[(Key, Value)]) -> Diff {
    // (change order, type, name, line)
    let mut changes: Vec<(u8, u64, &str, String)> = Vec::new();
    for ((kind, name), value) in local {
        let line = match remote
            .iter()
            .find(|(key, _)| key.0 == *kind && key.1 == *name)
        {
            None => (0, format!("+ create {} `{name}`", kind_name(*kind))),
            Some((_, theirs)) if theirs == value => continue,
            Some((_, theirs)) => (
                1,
                format!(
                    "~ update {} `{name}` ({})",
                    kind_name(*kind),
                    changed_keys(value, theirs).join(", ")
                ),
            ),
        };
        changes.push((line.0, *kind, name, line.1));
    }
    for ((kind, name), _) in remote {
        if !local
            .iter()
            .any(|(key, _)| key.0 == *kind && key.1 == *name)
        {
            changes.push((
                2,
                *kind,
                name,
                format!("- delete {} `{name}`", kind_name(*kind)),
            ));
        }
    }
    changes.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    let creates = changes.iter().filter(|c| c.0 == 0).count();
    Diff {
        lines: changes.into_iter().map(|c| c.3).collect(),
        creates,
    }
}
