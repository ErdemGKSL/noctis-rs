//! Click, select and modal-submit `data`, checked like Discord's client.

use serde_json::{Map, Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use super::wire::Resolved;
use crate::TestError;
use crate::response::{ModalForm, RunCtx, Setter, TestComponent, kind_name, kind_of};
use crate::seed::Seeds;

/// What a select run picks.
#[derive(Debug, Clone, Default)]
pub(crate) struct Picks {
    pub(crate) values: Vec<String>,
    pub(crate) users: Vec<Id<UserMarker>>,
    pub(crate) roles: Vec<Id<RoleMarker>>,
    pub(crate) channels: Vec<Id<ChannelMarker>>,
}

/// Checks that `user` may use `component` (enabled, visible to them).
fn usable(component: &TestComponent, user: Id<UserMarker>) -> Result<(), TestError> {
    if component.is_disabled() {
        return Err(TestError::Disabled {
            custom_id: component.custom_id().unwrap_or_default().to_owned(),
        });
    }
    if component.source.ephemeral && component.source.ctx.user != user {
        return Err(TestError::NotVisible { user: user.get() });
    }
    Ok(())
}

/// The `data` of a click on `component` by `user`.
pub(crate) fn click(component: &TestComponent, user: Id<UserMarker>) -> Result<Value, TestError> {
    if component.component_type() != 2 {
        return Err(TestError::WrongComponent {
            expected: "button",
            found: component.kind_name(),
        });
    }
    if matches!(
        component.json.get("style").and_then(Value::as_u64),
        Some(5 | 6)
    ) || component.custom_id().is_none()
    {
        return Err(TestError::NotInteractive);
    }
    usable(component, user)?;
    Ok(json!({
        "custom_id": component.custom_id(),
        "component_type": 2,
    }))
}

/// The `data` of a select on `component` by the run's user.
pub(crate) fn select(
    component: &TestComponent,
    picks: &Picks,
    seeds: &Seeds,
    ctx: &RunCtx,
) -> Result<Value, TestError> {
    let kind = component.component_type();
    if !matches!(kind, 3 | 5..=8) {
        return Err(TestError::WrongComponent {
            expected: "select",
            found: component.kind_name(),
        });
    }
    usable(component, ctx.user)?;
    let wrong = |expected| TestError::WrongComponent {
        expected,
        found: kind_name(kind),
    };
    let mut resolved = Resolved::default();
    let mut values: Vec<String> = Vec::new();
    if kind != 3 && !picks.values.is_empty() {
        return Err(wrong("string select"));
    }
    if !matches!(kind, 5 | 7) && !picks.users.is_empty() {
        return Err(wrong("user or mentionable select"));
    }
    if !matches!(kind, 6 | 7) && !picks.roles.is_empty() {
        return Err(wrong("role or mentionable select"));
    }
    if kind != 8 && !picks.channels.is_empty() {
        return Err(wrong("channel select"));
    }
    if kind == 3 {
        let options = component.options();
        if let Some(value) = picks.values.iter().find(|v| !options.contains(v)) {
            return Err(TestError::NotAnOption {
                value: value.clone(),
            });
        }
        values.clone_from(&picks.values);
    }
    for &user in &picks.users {
        resolved.user(seeds, ctx, user)?;
        values.push(user.to_string());
    }
    for &role in &picks.roles {
        resolved.role(seeds, ctx, role)?;
        values.push(role.to_string());
    }
    for &channel in &picks.channels {
        resolved.channel(seeds, ctx, channel)?;
        values.push(channel.to_string());
    }
    let bound = |key, default| {
        component
            .json
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or(default)
    };
    let (min, max) = (bound("min_values", 1), bound("max_values", 1));
    if values.len() < usize::from(min) || values.len() > usize::from(max) {
        return Err(TestError::SelectValues {
            min,
            max,
            got: values.len(),
        });
    }
    let mut data = json!({
        "custom_id": component.custom_id(),
        "component_type": kind,
        "values": values,
    });
    if let Some(resolved) = resolved.into_value() {
        data["resolved"] = resolved;
    }
    Ok(data)
}

/// The modal's inputs, Label-wrapped or in legacy Action Rows, with the
/// wrapper's `id`.
fn inputs(form: &ModalForm) -> Vec<(Option<&Value>, &Value)> {
    let mut out = Vec::new();
    for top in form
        .data
        .get("components")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
    {
        match kind_of(top) {
            Some(18) => {
                if let Some(input) = top.get("component") {
                    out.push((top.get("id"), input));
                }
            }
            Some(1) => {
                for input in top
                    .get("components")
                    .and_then(Value::as_array)
                    .map_or(&[][..], Vec::as_slice)
                {
                    out.push((None, input));
                }
            }
            _ => {}
        }
    }
    out
}

/// The kind name of input type `kind`.
const fn input_name(kind: u8) -> &'static str {
    match kind {
        4 => "text input",
        3 | 5..=8 => "select",
        19 => "file upload",
        21 => "radio group",
        22 => "checkbox group",
        23 => "checkbox",
        _ => "display component",
    }
}

/// The `data` of a submit of `form` by the run's user.
pub(crate) fn submit(form: &ModalForm, seeds: &Seeds, ctx: &RunCtx) -> Result<Value, TestError> {
    let inputs = inputs(form);
    let key_of = |input: &Value| {
        input
            .get("custom_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    for (key, setter) in &form.set {
        let Some((_, input)) = inputs
            .iter()
            .find(|(_, i)| key_of(i).as_deref() == Some(key))
        else {
            return Err(TestError::UnknownInput { key: key.clone() });
        };
        let kind = kind_of(input).unwrap_or(0);
        let fits = matches!(
            (kind, setter),
            (4, Setter::Text(_))
                | (3 | 5..=8 | 22, Setter::Values(_))
                | (21, Setter::Radio(_))
                | (23, Setter::Checkbox(_))
        );
        if kind == 19 {
            return Err(TestError::Unsupported(
                "file upload inputs [planned, Story 9.1]",
            ));
        }
        if !fits {
            return Err(TestError::WrongInput {
                key: key.clone(),
                expected: input_name(kind),
            });
        }
    }
    let mut resolved = Resolved::default();
    let mut components = Vec::new();
    for (wrapper_id, input) in inputs {
        let Some(submitted) = submitted(form, input, seeds, ctx, &mut resolved)? else {
            continue;
        };
        let mut label = json!({ "type": 18, "component": submitted });
        if let Some(id) = wrapper_id {
            label["id"] = id.clone();
        }
        components.push(label);
    }
    let mut data = json!({ "custom_id": form.custom_id(), "components": components });
    if let Some(resolved) = resolved.into_value() {
        data["resolved"] = resolved;
    }
    Ok(data)
}

/// What one input submits: the test's value, else its pre-filled one;
/// `None` for a component that submits nothing.
fn submitted(
    form: &ModalForm,
    input: &Value,
    seeds: &Seeds,
    ctx: &RunCtx,
    resolved: &mut Resolved,
) -> Result<Option<Value>, TestError> {
    let Some(key) = input.get("custom_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let latest = form
        .set
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, s)| s);
    let kind = kind_of(input).unwrap_or(0);
    let required = input.get("required").and_then(Value::as_bool) != Some(false);
    let mut out = Map::new();
    out.insert("type".to_owned(), json!(kind));
    if let Some(id) = input.get("id") {
        out.insert("id".to_owned(), id.clone());
    }
    out.insert("custom_id".to_owned(), json!(key));
    match kind {
        4 => {
            let value = match latest {
                Some(Setter::Text(v)) => v.clone(),
                _ => input
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            };
            check_text(key, &value, required, input)?;
            out.insert("value".to_owned(), json!(value));
        }
        3 | 5..=8 | 22 => {
            let values = match latest {
                Some(Setter::Values(v)) => v.clone(),
                _ => prefilled_values(input),
            };
            for value in &values {
                resolve_value(kind, value, seeds, ctx, resolved, key)?;
            }
            out.insert("values".to_owned(), json!(values));
        }
        21 => {
            let value = match latest {
                Some(Setter::Radio(v)) => Some(v.clone()),
                _ => prefilled_values(input).into_iter().next(),
            };
            out.insert("value".to_owned(), json!(value));
        }
        23 => {
            let value = match latest {
                Some(Setter::Checkbox(v)) => *v,
                _ => input
                    .get("default")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            out.insert("value".to_owned(), json!(value));
        }
        19 => {
            if required {
                return Err(TestError::Unsupported(
                    "file upload inputs [planned, Story 9.1]",
                ));
            }
            out.insert("values".to_owned(), json!([]));
        }
        _ => return Ok(None),
    }
    Ok(Some(Value::Object(out)))
}

/// Checks a text value like Discord's client: required, and within
/// `min_length..=max_length` in Discord's length unit when not empty.
fn check_text(key: &str, value: &str, required: bool, input: &Value) -> Result<(), TestError> {
    let len = noctis_rules::len::discord_len(value);
    let invalid = |reason: String| TestError::InvalidInput {
        key: key.to_owned(),
        reason,
    };
    if value.is_empty() {
        return if required {
            Err(invalid("required, but empty".to_owned()))
        } else {
            Ok(())
        };
    }
    let bound = |name| {
        input
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
    };
    if let Some(min) = bound("min_length")
        && len < min
    {
        return Err(invalid(format!("length {len} is under min_length {min}")));
    }
    if let Some(max) = bound("max_length")
        && len > max
    {
        return Err(invalid(format!("length {len} is over max_length {max}")));
    }
    Ok(())
}

/// The pre-filled values of a select, Checkbox Group or Radio Group:
/// options marked `default`, or `default_values` ids.
fn prefilled_values(input: &Value) -> Vec<String> {
    if let Some(options) = input.get("options").and_then(Value::as_array) {
        return options
            .iter()
            .filter(|o| o.get("default").and_then(Value::as_bool) == Some(true))
            .filter_map(|o| o.get("value").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
    }
    input
        .get("default_values")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Adds the `resolved` entry of one entity-select value.
fn resolve_value(
    kind: u8,
    value: &str,
    seeds: &Seeds,
    ctx: &RunCtx,
    resolved: &mut Resolved,
    key: &str,
) -> Result<(), TestError> {
    if !matches!(kind, 5..=8) {
        return Ok(());
    }
    let id = value
        .parse::<u64>()
        .ok()
        .filter(|&id| id != 0)
        .ok_or_else(|| TestError::InvalidInput {
            key: key.to_owned(),
            reason: format!("`{value}` is not an id"),
        })?;
    match kind {
        5 => resolved.user(seeds, ctx, Id::new(id)),
        6 => resolved.role(seeds, ctx, Id::new(id)),
        8 => resolved.channel(seeds, ctx, Id::new(id)),
        _ => {
            if seeds.users.contains_key(&Id::new(id)) {
                resolved.user(seeds, ctx, Id::new(id))
            } else {
                resolved.role(seeds, ctx, Id::new(id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use std::sync::Arc;

    use super::*;
    use crate::response::Source;
    use crate::seed::TestUser;

    fn ctx() -> RunCtx {
        RunCtx {
            user: Id::new(40),
            guild: None,
            channel: Id::new(40),
            channel_type: 1,
            locale: "en-US".to_owned(),
            guild_locale: None,
        }
    }

    fn seeds() -> Seeds {
        let mut seeds = Seeds::default();
        let a = TestUser::new(Id::new(40));
        seeds.users.insert(a.id, a);
        seeds
    }

    fn component(json: Value) -> TestComponent {
        TestComponent {
            json,
            source: Arc::new(Source {
                id: Id::new(5),
                json: Value::Null,
                ephemeral: false,
                ctx: ctx(),
            }),
        }
    }

    fn string_select() -> TestComponent {
        component(
            json!({ "type": 3, "custom_id": "4s:00", "min_values": 1, "max_values": 2,
            "options": [{ "label": "A", "value": "a" }, { "label": "B", "value": "b" }, { "label": "C", "value": "c" }] }),
        )
    }

    fn pick(values: &[&str]) -> Result<Value, TestError> {
        let picks = Picks {
            values: values.iter().map(|v| (*v).to_owned()).collect(),
            ..Picks::default()
        };
        select(&string_select(), &picks, &seeds(), &ctx())
    }

    /// X5, X6: select counts and option values.
    #[test]
    fn select_checks() {
        assert!(matches!(
            pick(&[]),
            Err(TestError::SelectValues {
                min: 1,
                max: 2,
                got: 0
            })
        ));
        assert_eq!(pick(&["a"]).unwrap()["values"], json!(["a"]));
        assert!(pick(&["a", "b"]).is_ok());
        assert!(matches!(
            pick(&["a", "b", "c"]),
            Err(TestError::SelectValues { got: 3, .. })
        ));
        assert!(matches!(
            pick(&["zzz"]),
            Err(TestError::NotAnOption { value }) if value == "zzz"
        ));
    }

    /// X2-X4: disabled, link and wrong-kind components.
    #[test]
    fn click_checks() {
        let user = Id::new(40);
        let disabled =
            component(json!({ "type": 2, "style": 1, "custom_id": "0a:1", "disabled": true }));
        assert!(
            matches!(click(&disabled, user), Err(TestError::Disabled { custom_id }) if custom_id == "0a:1")
        );
        let link = component(json!({ "type": 2, "style": 5, "url": "https://a.b" }));
        assert!(matches!(click(&link, user), Err(TestError::NotInteractive)));
        assert!(matches!(
            click(&string_select(), user),
            Err(TestError::WrongComponent {
                expected: "button",
                found: "string select"
            })
        ));
        let button = component(json!({ "type": 2, "style": 1, "custom_id": "0a:1" }));
        assert!(matches!(
            select(&button, &Picks::default(), &seeds(), &ctx()),
            Err(TestError::WrongComponent {
                expected: "select",
                found: "button"
            })
        ));
        assert_eq!(
            click(&button, user).unwrap(),
            json!({ "custom_id": "0a:1", "component_type": 2 })
        );
    }

    fn form() -> ModalForm {
        ModalForm {
            data: json!({ "custom_id": "0note:", "title": "Note", "components": [
                { "type": 18, "id": 1, "label": "Note", "component":
                    { "type": 4, "id": 2, "custom_id": "note", "style": 1, "max_length": 5 } },
                { "type": 18, "id": 3, "label": "Draft", "component":
                    { "type": 4, "id": 4, "custom_id": "draft", "style": 1, "required": false, "value": "pre" } },
            ]}),
            source: None,
            ctx: ctx(),
            set: Vec::new(),
        }
    }

    /// F2-F5: required, length bounds, unknown keys, wrong setters,
    /// pre-filled values.
    #[test]
    fn submit_checks() {
        let run = |form: ModalForm| submit(&form, &seeds(), &ctx());
        assert!(matches!(run(form()), Err(TestError::InvalidInput { key, .. }) if key == "note"));
        let ok = run(form().text("note", "12345")).unwrap();
        assert_eq!(
            ok["components"][0],
            json!({ "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "note", "value": "12345" } })
        );
        assert_eq!(ok["components"][1]["component"]["value"], "pre");
        assert!(matches!(
            run(form().text("note", "123456")),
            Err(TestError::InvalidInput { .. })
        ));
        assert!(matches!(
            run(form().text("note", "😀😀😀")),
            Err(TestError::InvalidInput { .. })
        ));
        assert!(matches!(
            run(form().text("note", "1").text("nope", "x")),
            Err(TestError::UnknownInput { key }) if key == "nope"
        ));
        assert!(matches!(
            run(form().checkbox("note", true)),
            Err(TestError::WrongInput {
                expected: "text input",
                ..
            })
        ));
    }
}
