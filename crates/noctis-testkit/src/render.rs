//! Snapshot JSON: [`render_json`] and the `custom_id` masking shared with
//! `message_json()`.

use noctis_ui::{CreateFlags, V2Message};
use serde_json::Value;

use crate::TestError;

/// The JSON a send of `msg` carries, without an App: a clone of `msg` whose
/// unresolved Route ids are resolved with placeholder parts, serialized as
/// `create_body(CreateFlags::empty())`, then masked like
/// [`Response::message_json`](crate::Response::message_json) (select nonce
/// `NN`, Ref `~RRRRRRRRRRR`, tag `TTTTTT`).
///
/// For a public V2 reply it equals the reply's `message_json()`, so a
/// render function can be snapshot without running it through an App.
///
/// ```
/// use noctis_ui::{Components, TextDisplay, V2Message};
///
/// let msg = V2Message::new(Components::new().push(TextDisplay::new("hi")));
/// let json = noctis_testkit::render_json(&msg)?;
/// assert_eq!(json["components"][0]["content"], "hi");
/// # Ok::<(), noctis_testkit::TestError>(())
/// ```
///
/// # Errors
///
/// [`TestError::Render`] when a Route id cannot be encoded or the body
/// cannot be serialized.
pub fn render_json(msg: &V2Message) -> Result<Value, TestError> {
    let mut msg = msg.clone();
    for id in msg.custom_ids_mut() {
        id.resolve_placeholder()
            .map_err(|e| TestError::Render(e.to_string()))?;
    }
    let body = serde_json::to_value(msg.create_body(CreateFlags::empty()))
        .map_err(|e| TestError::Render(e.to_string()))?;
    Ok(mask(&body))
}

/// `value` with every string `custom_id`, at any depth, masked.
pub(crate) fn mask(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, v)| {
                    let v = match (key.as_str(), v) {
                        ("custom_id", Value::String(raw)) => {
                            Value::String(noctis_core::__testing::mask_custom_id(raw).into_owned())
                        }
                        _ => mask(v),
                    };
                    (key.clone(), v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(mask).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// U1: inline unsigned ids and foreign ids are unchanged; volatile
    /// parts are masked at any depth.
    #[test]
    fn masking_keeps_stable_ids() {
        let body = json!({ "components": [
            { "type": 2, "custom_id": "1turn:5" },
            { "type": 2, "custom_id": "abc" },
            { "type": 1, "components": [{ "type": 3, "custom_id": "4pick:z0a" }] },
            { "type": 2, "custom_id": "0big:~0123456789A" },
        ]});
        assert_eq!(
            mask(&body),
            json!({ "components": [
                { "type": 2, "custom_id": "1turn:5" },
                { "type": 2, "custom_id": "abc" },
                { "type": 1, "components": [{ "type": 3, "custom_id": "4pick:NNa" }] },
                { "type": 2, "custom_id": "0big:~RRRRRRRRRRR" },
            ]})
        );
    }

    /// M12: an empty V2 message.
    #[test]
    fn empty_message_renders() -> Result<(), TestError> {
        let msg = V2Message::new(noctis_ui::Components::new());
        assert_eq!(
            render_json(&msg)?,
            json!({ "flags": 32768, "components": [], "allowed_mentions": { "parse": [] } })
        );
        Ok(())
    }
}
