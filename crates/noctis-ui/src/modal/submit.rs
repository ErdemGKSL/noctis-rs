//! Modal submissions decoded from the raw interaction JSON:
//! [`ModalSubmission`], [`SubmittedValue`] and [`SubmitError`] (FR-64,
//! MOD-09; AD-12).
//!
//! twilight-model 0.17.1 turns Radio Groups (21), Checkbox Groups (22) and
//! Checkboxes (23) into `ModalInteractionComponent::Unknown` and drops their
//! values, so the components are walked here from `serde_json::Value`; only
//! `data.resolved` goes through twilight's `InteractionDataResolved`.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};
use twilight_model::application::interaction::InteractionDataResolved;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    AttachmentMarker, ChannelMarker, GenericMarker, RoleMarker, UserMarker,
};

/// Why a modal submission could not be decoded (MOD-09). Every variant
/// names the input key it concerns, except the whole-payload ones.
///
/// ```
/// use noctis_ui::{ModalSubmission, SubmitError};
///
/// let err = ModalSubmission::from_slice(br#"{"type": 3, "data": {}}"#).unwrap_err();
/// assert_eq!(err, SubmitError::NotModalSubmit { kind: 3 });
/// assert!(err.to_string().starts_with("[MOD-09]"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubmitError {
    /// The payload is not a modal submission Noctis can read: bad JSON, a
    /// missing or ill-typed `type`, `data`, `custom_id` or `components`, an
    /// input without a string `custom_id`, or bad `resolved` data.
    #[error("[MOD-09] malformed modal submission: {reason}")]
    Malformed {
        /// What is wrong.
        reason: String,
    },
    /// The interaction is not a modal submission (`type` is not 5).
    #[error("[MOD-09] not a modal submission: interaction type {kind}")]
    NotModalSubmit {
        /// The interaction `type` found.
        kind: u64,
    },
    /// Two inputs share a key.
    #[error("[MOD-09] `{key}` was submitted twice")]
    Duplicate {
        /// The repeated key.
        key: String,
    },
    /// A required input is absent, or empty on a single-valued input (an
    /// empty required `Vec` is [`Invalid`](Self::Invalid): its count is
    /// below the minimum).
    #[error("[MOD-09] `{key}` is required but has no value")]
    Missing {
        /// The input key.
        key: String,
    },
    /// The input has another component type than the form declares.
    #[error("[MOD-09] `{key}`: expected component type {expected}, found {found}")]
    WrongType {
        /// The input key.
        key: String,
        /// The declared Discord component type.
        expected: u64,
        /// The submitted component type.
        found: u64,
    },
    /// The value is ill-typed or outside the declared bounds: a bad
    /// snowflake, an unknown choice value, a length or count out of range.
    #[error("[MOD-09] `{key}`: {reason}")]
    Invalid {
        /// The input key.
        key: String,
        /// What is wrong.
        reason: String,
    },
    /// A submitted id has no entry in the submission's resolved data.
    #[error("[MOD-09] `{key}`: id {id} is not in the resolved data")]
    Unresolved {
        /// The input key.
        key: String,
        /// The missing id.
        id: u64,
    },
}

impl SubmitError {
    fn malformed(reason: impl Into<String>) -> Self {
        Self::Malformed {
            reason: reason.into(),
        }
    }

    pub(crate) fn invalid(key: &str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            key: key.to_owned(),
            reason: reason.into(),
        }
    }

    pub(crate) fn missing(key: &str) -> Self {
        Self::Missing {
            key: key.to_owned(),
        }
    }
}

/// The value one input submitted, keyed by its `custom_id` in a
/// [`ModalSubmission`].
///
/// ```
/// use noctis_ui::{ModalSubmission, SubmittedValue};
///
/// let json = br#"{"type": 5, "data": {"custom_id": "m", "components": [
///     {"type": 18, "component": {"type": 23, "custom_id": "ok", "value": true}}
/// ]}}"#;
/// let submission = ModalSubmission::from_slice(json)?;
/// assert_eq!(submission.get("ok"), Some(&SubmittedValue::Checkbox(true)));
/// # Ok::<(), noctis_ui::SubmitError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SubmittedValue {
    /// A Text Input (type 4) `value`.
    TextInput(String),
    /// A String Select (type 3) `values`.
    StringSelect(Vec<String>),
    /// A User Select (type 5) `values`.
    UserSelect(Vec<Id<UserMarker>>),
    /// A Role Select (type 6) `values`.
    RoleSelect(Vec<Id<RoleMarker>>),
    /// A Mentionable Select (type 7) `values`: user or role ids.
    MentionableSelect(Vec<Id<GenericMarker>>),
    /// A Channel Select (type 8) `values`.
    ChannelSelect(Vec<Id<ChannelMarker>>),
    /// A File Upload (type 19) `values`: attachment ids.
    FileUpload(Vec<Id<AttachmentMarker>>),
    /// A Radio Group (type 21) `value`; `None` when nothing was chosen.
    RadioGroup(Option<String>),
    /// A Checkbox Group (type 22) `values`.
    CheckboxGroup(Vec<String>),
    /// A Checkbox (type 23) `value`.
    Checkbox(bool),
}

impl SubmittedValue {
    /// The Discord component type the value was submitted as.
    pub(crate) fn component_type(&self) -> u64 {
        match self {
            Self::TextInput(_) => 4,
            Self::StringSelect(_) => 3,
            Self::UserSelect(_) => 5,
            Self::RoleSelect(_) => 6,
            Self::MentionableSelect(_) => 7,
            Self::ChannelSelect(_) => 8,
            Self::FileUpload(_) => 19,
            Self::RadioGroup(_) => 21,
            Self::CheckboxGroup(_) => 22,
            Self::Checkbox(_) => 23,
        }
    }
}

/// A modal submission (interaction type 5) decoded from the raw interaction
/// JSON: the modal's `custom_id`, every input value by key, the resolved
/// users, members, roles, channels and attachments, and every component
/// Noctis does not know, kept verbatim.
///
/// Label-wrapped (type 18) and legacy Action-Row-wrapped (type 1) inputs are
/// both read; Text Displays (type 10) are skipped. Decoding never checks the
/// `custom_id` against a Route: dispatch is core's. Every value is untrusted
/// (MOD-09). A [`ModalForm`](crate::ModalForm) is decoded from it.
///
/// ```
/// use noctis_ui::{ModalSubmission, SubmittedValue};
///
/// let json = br#"{"type": 5, "data": {"custom_id": "0form.save:", "components": [
///     {"type": 18, "component": {"type": 4, "custom_id": "bio", "value": "hi"}},
///     {"type": 18, "component": {"type": 21, "custom_id": "sev", "value": null}},
///     {"type": 99, "custom_id": "x", "foo": 1}
/// ]}}"#;
/// let submission = ModalSubmission::from_slice(json)?;
/// assert_eq!(submission.custom_id(), "0form.save:");
/// assert_eq!(submission.get("bio"), Some(&SubmittedValue::TextInput("hi".into())));
/// assert_eq!(submission.get("sev"), Some(&SubmittedValue::RadioGroup(None)));
/// assert_eq!(submission.unknown()[0]["foo"], 1);
/// # Ok::<(), noctis_ui::SubmitError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ModalSubmission {
    custom_id: String,
    values: BTreeMap<String, SubmittedValue>,
    resolved: InteractionDataResolved,
    unknown: Vec<Value>,
}

impl ModalSubmission {
    /// Decodes a whole interaction object.
    ///
    /// # Errors
    ///
    /// [`SubmitError::NotModalSubmit`] when `type` is not 5,
    /// [`SubmitError::Duplicate`] when two inputs share a key,
    /// [`SubmitError::Invalid`] for an ill-typed value and
    /// [`SubmitError::Malformed`] for anything else unreadable. It never
    /// panics.
    ///
    /// ```
    /// use noctis_ui::{ModalSubmission, SubmitError};
    ///
    /// assert!(matches!(ModalSubmission::from_slice(b"not json"), Err(SubmitError::Malformed { .. })));
    /// ```
    pub fn from_slice(bytes: &[u8]) -> Result<Self, SubmitError> {
        let root: Value = serde_json::from_slice(bytes)
            .map_err(|e| SubmitError::malformed(format!("invalid JSON: {e}")))?;
        let Value::Object(mut root) = root else {
            return Err(SubmitError::malformed("the interaction is not an object"));
        };
        let kind = root
            .get("type")
            .and_then(Value::as_u64)
            .ok_or_else(|| SubmitError::malformed("`type` is missing or not an integer"))?;
        if kind != 5 {
            return Err(SubmitError::NotModalSubmit { kind });
        }
        let Some(Value::Object(mut data)) = root.remove("data") else {
            return Err(SubmitError::malformed("`data` is missing or not an object"));
        };
        let Some(Value::String(custom_id)) = data.remove("custom_id") else {
            return Err(SubmitError::malformed(
                "`data.custom_id` is missing or not a string",
            ));
        };
        let Some(Value::Array(components)) = data.remove("components") else {
            return Err(SubmitError::malformed(
                "`data.components` is missing or not an array",
            ));
        };
        let mut this = Self {
            custom_id,
            values: BTreeMap::new(),
            resolved: empty_resolved(),
            unknown: Vec::new(),
        };
        if let Some(resolved) = data.remove("resolved") {
            this.merge_resolved(resolved)?;
        }
        for component in components {
            this.walk(component)?;
        }
        Ok(this)
    }

    /// The modal's `custom_id` (its Route's encoding).
    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    /// The value submitted under `key`, if any.
    pub fn get(&self, key: &str) -> Option<&SubmittedValue> {
        self.values.get(key)
    }

    /// The resolved users, members, roles, channels and attachments of the
    /// submission (`data.resolved` and any per-component `resolved`).
    pub fn resolved(&self) -> &InteractionDataResolved {
        &self.resolved
    }

    /// The components Noctis does not know (any type but 1, 3-8, 10, 18,
    /// 19, 21-23, or a non-object), verbatim.
    pub fn unknown(&self) -> &[Value] {
        &self.unknown
    }

    /// The `type` of an unknown component submitted under `key`.
    pub(crate) fn unknown_type_of(&self, key: &str) -> Option<u64> {
        self.unknown.iter().find_map(|c| {
            (c.get("custom_id").and_then(Value::as_str) == Some(key))
                .then(|| c.get("type").and_then(Value::as_u64).unwrap_or(0))
        })
    }

    fn merge_resolved(&mut self, resolved: Value) -> Result<(), SubmitError> {
        if resolved.is_null() {
            return Ok(());
        }
        let r: InteractionDataResolved = serde_json::from_value(resolved)
            .map_err(|e| SubmitError::malformed(format!("bad `resolved` data: {e}")))?;
        self.resolved.attachments.extend(r.attachments);
        self.resolved.channels.extend(r.channels);
        self.resolved.members.extend(r.members);
        self.resolved.messages.extend(r.messages);
        self.resolved.roles.extend(r.roles);
        self.resolved.users.extend(r.users);
        Ok(())
    }

    fn walk(&mut self, component: Value) -> Result<(), SubmitError> {
        let Value::Object(mut obj) = component else {
            self.unknown.push(component);
            return Ok(());
        };
        let Some(ty) = obj.get("type").and_then(Value::as_u64) else {
            self.unknown.push(Value::Object(obj));
            return Ok(());
        };
        match ty {
            18 => match obj.remove("component") {
                Some(child) => self.walk(child),
                None => Err(SubmitError::malformed(
                    "a Label (type 18) without `component`",
                )),
            },
            1 => match obj.remove("components") {
                Some(Value::Array(children)) => children.into_iter().try_for_each(|c| self.walk(c)),
                _ => Err(SubmitError::malformed(
                    "an Action Row (type 1) without a `components` array",
                )),
            },
            10 => Ok(()),
            3..=8 | 19 | 21..=23 => {
                let Some(Value::String(key)) = obj.remove("custom_id") else {
                    return Err(SubmitError::malformed(format!(
                        "a type {ty} component without a string `custom_id`"
                    )));
                };
                if let Some(resolved) = obj.remove("resolved") {
                    self.merge_resolved(resolved)?;
                }
                let value = parse_value(ty, &key, &mut obj)?;
                if self.values.contains_key(&key) {
                    return Err(SubmitError::Duplicate { key });
                }
                self.values.insert(key, value);
                Ok(())
            }
            _ => {
                self.unknown.push(Value::Object(obj));
                Ok(())
            }
        }
    }
}

fn empty_resolved() -> InteractionDataResolved {
    InteractionDataResolved {
        attachments: HashMap::new(),
        channels: HashMap::new(),
        members: HashMap::new(),
        messages: HashMap::new(),
        roles: HashMap::new(),
        users: HashMap::new(),
    }
}

/// The value of a known input type (3-8, 19, 21-23).
fn parse_value(
    ty: u64,
    key: &str,
    obj: &mut Map<String, Value>,
) -> Result<SubmittedValue, SubmitError> {
    Ok(match ty {
        4 => match obj.remove("value") {
            Some(Value::String(s)) => SubmittedValue::TextInput(s),
            _ => return Err(SubmitError::invalid(key, "`value` is not a string")),
        },
        3 => SubmittedValue::StringSelect(strings(key, obj)?),
        5 => SubmittedValue::UserSelect(ids(key, obj)?),
        6 => SubmittedValue::RoleSelect(ids(key, obj)?),
        7 => SubmittedValue::MentionableSelect(ids(key, obj)?),
        8 => SubmittedValue::ChannelSelect(ids(key, obj)?),
        19 => SubmittedValue::FileUpload(ids(key, obj)?),
        21 => match obj.remove("value") {
            None | Some(Value::Null) => SubmittedValue::RadioGroup(None),
            Some(Value::String(s)) => SubmittedValue::RadioGroup(Some(s)),
            Some(_) => {
                return Err(SubmitError::invalid(key, "`value` is not a string or null"));
            }
        },
        22 => SubmittedValue::CheckboxGroup(strings(key, obj)?),
        23 => match obj.remove("value") {
            Some(Value::Bool(b)) => SubmittedValue::Checkbox(b),
            _ => return Err(SubmitError::invalid(key, "`value` is not a boolean")),
        },
        _ => return Err(SubmitError::malformed(format!("type {ty} has no value"))),
    })
}

/// The `values` array as strings.
fn strings(key: &str, obj: &mut Map<String, Value>) -> Result<Vec<String>, SubmitError> {
    let Some(Value::Array(values)) = obj.remove("values") else {
        return Err(SubmitError::invalid(key, "`values` is not an array"));
    };
    values
        .into_iter()
        .map(|v| match v {
            Value::String(s) => Ok(s),
            _ => Err(SubmitError::invalid(key, "`values` holds a non-string")),
        })
        .collect()
}

/// The `values` array as snowflakes.
fn ids<M>(key: &str, obj: &mut Map<String, Value>) -> Result<Vec<Id<M>>, SubmitError> {
    strings(key, obj)?
        .iter()
        .map(|s| {
            s.parse::<u64>()
                .ok()
                .and_then(Id::new_checked)
                .ok_or_else(|| SubmitError::invalid(key, format!("`{s}` is not a snowflake")))
        })
        .collect()
}

#[cfg(test)]
mod tests;
