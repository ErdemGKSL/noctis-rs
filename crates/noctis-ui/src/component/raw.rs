//! The raw escape hatch (FR-31, CV2-16): [`RawComponent`], a checked,
//! verbatim JSON component, and [`unknown_components`], the incoming
//! components Noctis does not model.

use serde::{Serialize, Serializer};
use serde_json::Value;

use noctis_rules::{Actual, Bound, RuleId, Violation};

use crate::attachment::SCHEME;
use crate::error::{UiError, checked};
use crate::route::CustomId;
use crate::wire::{Ids, Wire, serialize_via_wire};

/// The deepest a raw node may nest, the root being level 1 (CV2-16).
const MAX_DEPTH: usize = 32;

/// The CV2-16 bound of a node that is not a component object.
const COMPONENT_OBJECT: Bound =
    Bound::Describe("a component object: a JSON object with an integer `type` of at least 1");

/// The CV2-14 bound of an `id` key.
const NO_ID: Bound = Bound::Describe("no `id`: Noctis numbers components (CV2-14)");

/// The CV2-14 help of an `id` key.
const NO_ID_HELP: &str = "remove `id`; Discord numbers raw components itself";

/// A component Noctis does not model, as raw Discord JSON that is sent
/// verbatim (FR-31, CV2-16).
///
/// [`RawComponent::new`] checks the node once, and there is no mutation
/// API, so the check holds for the node's whole life. A *component object*
/// is a JSON object whose `type` is an integer of at least 1. The check
/// walks the root and, below it, only the `components` array, the
/// `accessory` and the `component` keys; every other key (`default_values`,
/// `emoji`, `options`, `items`, …) is left alone. It rejects, collecting
/// every violation in pre-order:
///
/// - a walked node that is not a component object, or a `components` that
///   is not an array (CV2-16);
/// - an `id` key on any walked component object (CV2-14): Noctis numbers
///   its own components, so a raw node never carries one and Discord
///   numbers it;
/// - nesting deeper than 32 levels (CV2-16).
///
/// Placement: the top level of a message or a Container, never a Section
/// accessory, an Action Row or a modal. It counts [`count`](Self::count)
/// components toward CV2-03 (in `validate`, `cost` and `Budget`), its text
/// is not counted toward CV2-04, and Lenient mode never changes it. Every
/// `attachment://` string inside it is tracked like a [`Media`](crate::Media)
/// URL (ATT-05, uploads). `custom_id`s inside it are not checked (CID-02).
///
/// Discord may change or reject component types Noctis does not model;
/// prefer the typed components whenever one exists.
///
/// ```
/// use noctis_ui::{Components, RawComponent, TextDisplay};
/// use serde_json::json;
///
/// let poll = RawComponent::new(json!({"type": 99, "question": "Lunch?"}))?;
/// assert_eq!((poll.component_type(), poll.count()), (99, 1));
/// let body = Components::new()
///     .push(TextDisplay::new("a"))
///     .push(poll)
///     .push(TextDisplay::new("b"));
/// assert_eq!(
///     serde_json::to_value(&body).unwrap(),
///     json!([
///         {"type": 10, "id": 1, "content": "a"},
///         {"type": 99, "question": "Lunch?"},
///         {"type": 10, "id": 2, "content": "b"}
///     ])
/// );
/// # Ok::<(), noctis_ui::UiError>(())
/// ```
///
/// An `id` or a bad child is rejected with its path:
///
/// ```
/// use noctis_rules::RuleId;
/// use noctis_ui::{RawComponent, UiError};
/// use serde_json::json;
///
/// let Err(UiError::Limits { violations }) =
///     RawComponent::new(json!({"type": 99, "id": 5, "components": [1]}))
/// else {
///     panic!("rejected");
/// };
/// assert_eq!((violations[0].rule, violations[0].path.as_str()), (RuleId::CV2_14, "id"));
/// assert_eq!((violations[1].rule, violations[1].path.as_str()), (RuleId::CV2_16, "components[0]"));
/// ```
///
/// A raw node is not a Section accessory (`[CV2-16]`):
///
/// ```compile_fail,E0277
/// use noctis_ui::{RawComponent, Section, TextDisplay};
///
/// let raw = RawComponent::new(serde_json::json!({"type": 99})).unwrap();
/// let _ = Section::new([TextDisplay::new("a")], raw); // [CV2-16]
/// ```
///
/// nor an Action Row item (`[CV2-16]`):
///
/// ```compile_fail,E0277
/// use noctis_ui::{ActionRow, RawComponent};
///
/// let raw = RawComponent::new(serde_json::json!({"type": 99})).unwrap();
/// let _ = ActionRow::buttons([raw]); // [CV2-16]
/// ```
///
/// nor a modal input (`[CV2-16]`):
///
/// ```compile_fail,E0277
/// use noctis_ui::{ModalLabel, RawComponent};
///
/// let raw = RawComponent::new(serde_json::json!({"type": 99})).unwrap();
/// let _ = ModalLabel::new("a", raw); // [CV2-16]
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RawComponent {
    value: Value,
    count: usize,
}

impl RawComponent {
    /// Checks `value` (see the type docs) and wraps it unchanged.
    ///
    /// # Errors
    ///
    /// [`UiError::Limits`] with every CV2-14 and CV2-16 violation, in
    /// pre-order.
    pub fn new(value: Value) -> Result<Self, UiError> {
        let mut check = Check::default();
        check.node(&value, String::new(), 1);
        let count = check.count;
        checked(Self { value, count }, check.violations)
    }

    /// The JSON, exactly as given.
    pub fn as_json(&self) -> &Value {
        &self.value
    }

    /// The JSON, exactly as given.
    pub fn into_json(self) -> Value {
        self.value
    }

    /// The root's `type`.
    pub fn component_type(&self) -> u64 {
        // The check proved the root is a component object.
        component_type(&self.value).unwrap_or_default()
    }

    /// The components this node counts toward CV2-03: every component
    /// object of the walk, the root included.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Pushes one WH-02 violation per interactive node of the walk (the
    /// keys the construction check walks: `components`, `accessory`,
    /// `component`): an object whose integer `type` is a select (3, 5, 6, 7,
    /// 8), or a button (2) whose `style` is not 5 (link). Paths are
    /// `<path>.<json path>` (`path` alone for the root). For
    /// [`StaticMessage::try_from`](crate::StaticMessage).
    pub(crate) fn static_violations(&self, path: &str, out: &mut Vec<Violation>) {
        static_walk(&self.value, path, out);
    }

    /// Every string value that starts with `attachment://`, at any key and
    /// depth, in document order.
    pub(crate) fn attachment_urls(&self) -> Vec<&str> {
        let mut out = Vec::new();
        // An explicit stack: keys the check does not walk may nest deeply.
        let mut stack = vec![&self.value];
        while let Some(value) = stack.pop() {
            match value {
                Value::String(s) if s.starts_with(SCHEME) => out.push(s.as_str()),
                Value::Array(items) => stack.extend(items.iter().rev()),
                Value::Object(map) => stack.extend(map.values().rev()),
                _ => {}
            }
        }
        out
    }
}

impl TryFrom<Value> for RawComponent {
    type Error = UiError;

    /// [`RawComponent::new`].
    fn try_from(value: Value) -> Result<Self, UiError> {
        Self::new(value)
    }
}

impl Wire for RawComponent {
    /// The stored JSON; never consumes an id.
    fn wire<S: Serializer>(&self, _ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(s)
    }

    fn custom_ids_mut<'a>(&'a mut self, _out: &mut Vec<&'a mut CustomId>) {}
}

serialize_via_wire!(RawComponent);

/// The integer `type` of a component object (at least 1).
fn component_type(value: &Value) -> Option<u64> {
    value.as_object()?.get("type")?.as_u64().filter(|&t| t >= 1)
}

/// The JSON kind of a node that is not a component object.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
        Value::Object(_) => "object without a valid type",
    }
}

/// `parent.child`, or `child` at the root.
fn join(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}.{child}")
    }
}

/// The pre-order check of [`RawComponent::new`].
#[derive(Default)]
struct Check {
    count: usize,
    violations: Vec<Violation>,
}

impl Check {
    fn node(&mut self, value: &Value, path: String, depth: usize) {
        if depth > MAX_DEPTH {
            self.violations.push(Violation::new(
                RuleId::CV2_16,
                path,
                Bound::AtMost(MAX_DEPTH),
                Actual::Count(depth),
            ));
            return;
        }
        let (Some(map), Some(_)) = (value.as_object(), component_type(value)) else {
            self.violations.push(Violation::new(
                RuleId::CV2_16,
                path,
                COMPONENT_OBJECT,
                Actual::Text(kind(value).to_owned()),
            ));
            return;
        };
        self.count += 1;
        if let Some(id) = map.get("id") {
            self.violations.push(
                Violation::new(
                    RuleId::CV2_14,
                    join(&path, "id"),
                    NO_ID,
                    Actual::Text(id.to_string()),
                )
                .with_help(NO_ID_HELP),
            );
        }
        if let Some(children) = map.get("components") {
            match children.as_array() {
                Some(children) => {
                    for (i, child) in children.iter().enumerate() {
                        self.node(child, join(&path, &format!("components[{i}]")), depth + 1);
                    }
                }
                None => self.violations.push(Violation::new(
                    RuleId::CV2_16,
                    join(&path, "components"),
                    COMPONENT_OBJECT,
                    Actual::Text(kind(children).to_owned()),
                )),
            }
        }
        for key in ["accessory", "component"] {
            if let Some(child) = map.get(key) {
                self.node(child, join(&path, key), depth + 1);
            }
        }
    }
}

/// The WH-02 bound of an interactive raw node.
const STATIC_ONLY: Bound = Bound::Describe(
    "a non-interactive component (a link button, text, media, file, separator, container or section)",
);

/// The walk of [`RawComponent::static_violations`]; the construction check
/// already bounded its depth.
fn static_walk(value: &Value, path: &str, out: &mut Vec<Violation>) {
    let Some(map) = value.as_object() else {
        return;
    };
    let kind = match map.get("type").and_then(Value::as_u64) {
        Some(3 | 5..=8) => Some("select"),
        Some(2) if map.get("style").and_then(Value::as_u64) != Some(5) => Some("button"),
        _ => None,
    };
    if let Some(kind) = kind {
        out.push(Violation::new(
            RuleId::WH_02,
            path.to_owned(),
            STATIC_ONLY,
            Actual::Text(kind.to_owned()),
        ));
    }
    if let Some(children) = map.get("components").and_then(Value::as_array) {
        for (i, child) in children.iter().enumerate() {
            static_walk(child, &join(path, &format!("components[{i}]")), out);
        }
    }
    for key in ["accessory", "component"] {
        if let Some(child) = map.get(key) {
            static_walk(child, &join(path, key), out);
        }
    }
}

/// The message component types Noctis models.
const MODELED: [u64; 14] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17];

/// The components of `components` Noctis does not model, cloned whole, in
/// pre-order (FR-31, CV2-16).
///
/// The walk descends into the `components` of Action Rows (1), Sections
/// (9) and Containers (17) and into a Section's `accessory`. Every element
/// that is not an object with a modeled integer `type` (1, 2, 3, 5-14 or
/// 17) is collected: unknown types, a missing or non-integer `type`, and
/// non-objects. An unknown node is taken whole and its subtree is not
/// walked; a `components` or `accessory` of the wrong JSON shape inside a
/// known node is skipped.
///
/// ```
/// use noctis_ui::unknown_components;
/// use serde_json::json;
///
/// let message = [
///     json!({"type": 10, "content": "a"}),
///     json!({"type": 17, "components": [{"type": 98}]}),
///     json!({"type": 9, "components": [], "accessory": {"type": 97}}),
/// ];
/// assert_eq!(unknown_components(&message), [json!({"type": 98}), json!({"type": 97})]);
/// ```
pub fn unknown_components(components: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    collect_unknown(components, &mut out);
    out
}

fn collect_unknown(components: &[Value], out: &mut Vec<Value>) {
    for node in components {
        let Some(ty) = node
            .as_object()
            .and_then(|m| m.get("type"))
            .and_then(Value::as_u64)
            .filter(|t| MODELED.contains(t))
        else {
            out.push(node.clone());
            continue;
        };
        if matches!(ty, 1 | 9 | 17)
            && let Some(children) = node.get("components").and_then(Value::as_array)
        {
            collect_unknown(children, out);
        }
        if ty == 9
            && let Some(accessory) = node.get("accessory").filter(|a| a.is_object())
        {
            collect_unknown(core::slice::from_ref(accessory), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{Components, Container, TextDisplay};

    fn violations(value: Value) -> Vec<Violation> {
        match RawComponent::new(value) {
            Err(UiError::Limits { violations }) => violations,
            other => panic!("expected a limits error, got {other:?}"),
        }
    }

    fn summary(v: &[Violation]) -> Vec<(RuleId, &str)> {
        v.iter().map(|v| (v.rule, v.path.as_str())).collect()
    }

    fn count(value: Value) -> usize {
        RawComponent::new(value).expect("a valid raw node").count()
    }

    fn static_paths(value: Value) -> Vec<(RuleId, String)> {
        let mut out = Vec::new();
        RawComponent::new(value)
            .expect("a valid raw node")
            .static_violations("opaque[0]", &mut out);
        out.into_iter().map(|v| (v.rule, v.path)).collect()
    }

    #[test]
    fn wh_02_raw_static_walk() {
        assert_eq!(
            static_paths(
                json!({"type": 1, "components": [{"type": 2, "style": 1, "custom_id": "x"}]})
            ),
            [(RuleId::WH_02, "opaque[0].components[0]".to_owned())]
        );
        assert!(
            static_paths(
                json!({"type": 1, "components": [{"type": 2, "style": 5, "url": "https://a.b"}]})
            )
            .is_empty()
        );
        assert_eq!(
            static_paths(json!({"type": 17, "components": [{"type": 3, "custom_id": "s"}]})),
            [(RuleId::WH_02, "opaque[0].components[0]".to_owned())]
        );
        assert!(static_paths(json!({"type": 10, "content": "a"})).is_empty());
        assert_eq!(
            static_paths(json!({"type": 2, "style": 6, "sku_id": "1"})),
            [(RuleId::WH_02, "opaque[0]".to_owned())]
        );
        assert_eq!(
            static_paths(
                json!({"type": 9, "components": [], "accessory": {"type": 2, "style": 2}})
            ),
            [(RuleId::WH_02, "opaque[0].accessory".to_owned())]
        );
        for kind in 5..=8 {
            assert_eq!(
                static_paths(json!({"type": 1, "components": [{"type": kind, "custom_id": "u"}]})),
                [(RuleId::WH_02, "opaque[0].components[0]".to_owned())],
                "select type {kind}"
            );
        }
        assert_eq!(
            static_paths(json!({"type": 1, "components": [{"type": 2, "custom_id": "x"}]})),
            [(RuleId::WH_02, "opaque[0].components[0]".to_owned())]
        );
    }

    #[test]
    fn cv2_16_raw_minimal() {
        let raw = RawComponent::new(json!({"type": 99})).unwrap();
        assert_eq!((raw.count(), raw.component_type()), (1, 99));
        assert_eq!(RawComponent::try_from(json!({"type": 99})), Ok(raw));
    }

    #[test]
    fn cv2_16_raw_nested_counts() {
        assert_eq!(
            count(json!({"type": 99, "components": [{"type": 10, "content": "a"}, {"type": 98}]})),
            3
        );
        assert_eq!(
            count(
                json!({"type": 9, "components": [{"type": 10, "content": "a"}], "accessory": {"type": 97}})
            ),
            3
        );
        assert_eq!(count(json!({"type": 18, "component": {"type": 4}})), 2);
        assert_eq!(count(json!({"type": 99, "components": []})), 1);
    }

    #[test]
    fn cv2_16_raw_root_not_object() {
        for (value, actual) in [
            (json!([]), "array"),
            (json!("x"), "string"),
            (json!(1), "number"),
            (json!(true), "bool"),
            (json!(null), "null"),
        ] {
            let v = violations(value);
            assert_eq!(summary(&v), [(RuleId::CV2_16, "")]);
            assert_eq!(v[0].limit, COMPONENT_OBJECT);
            assert_eq!(v[0].actual, Actual::Text(actual.to_owned()));
        }
    }

    #[test]
    fn cv2_16_raw_bad_type() {
        for bad in [
            json!({}),
            json!({"type": 0}),
            json!({"type": -1}),
            json!({"type": 1.5}),
            json!({"type": "9"}),
            json!({"type": null}),
        ] {
            let v = violations(bad.clone());
            assert_eq!(summary(&v), [(RuleId::CV2_16, "")], "{bad}");
            assert_eq!(
                v[0].actual,
                Actual::Text("object without a valid type".to_owned())
            );
        }
        let max = RawComponent::new(json!({"type": u64::MAX})).unwrap();
        assert_eq!(max.component_type(), u64::MAX);
    }

    #[test]
    fn cv2_16_raw_components_not_array() {
        let v = violations(json!({"type": 99, "components": {}}));
        assert_eq!(summary(&v), [(RuleId::CV2_16, "components")]);
        assert_eq!(
            v[0].actual,
            Actual::Text("object without a valid type".to_owned())
        );
    }

    #[test]
    fn cv2_16_raw_bad_children() {
        assert_eq!(
            summary(&violations(json!({"type": 99, "components": [1]}))),
            [(RuleId::CV2_16, "components[0]")]
        );
        assert_eq!(
            summary(&violations(json!({"type": 9, "accessory": null}))),
            [(RuleId::CV2_16, "accessory")]
        );
        assert_eq!(
            summary(&violations(json!({"type": 18, "component": "x"}))),
            [(RuleId::CV2_16, "component")]
        );
    }

    #[test]
    fn cv2_14_raw_id_rejected() {
        for id in [json!(5), json!(0), json!(null)] {
            let v = violations(json!({"type": 99, "id": id.clone()}));
            assert_eq!(summary(&v), [(RuleId::CV2_14, "id")]);
            assert_eq!(v[0].limit, NO_ID);
            assert_eq!(v[0].actual, Actual::Text(id.to_string()));
            assert_eq!(v[0].help.as_deref(), Some(NO_ID_HELP));
        }
        let v =
            violations(json!({"type": 99, "components": [{"type": 10}, {"type": 10, "id": 3}]}));
        assert_eq!(summary(&v), [(RuleId::CV2_14, "components[1].id")]);
        let v = violations(
            json!({"type": 99, "components": [{"type": 9, "accessory": {"type": 2, "id": 1}}]}),
        );
        assert_eq!(
            summary(&v),
            [(RuleId::CV2_14, "components[0].accessory.id")]
        );
    }

    #[test]
    fn cv2_14_raw_ids_outside_components_pass() {
        let raw = json!({
            "type": 5,
            "default_values": [{"id": "1", "type": "user"}],
            "emoji": {"id": "2"},
        });
        assert_eq!(count(raw), 1);
    }

    #[test]
    fn cv2_16_raw_depth() {
        let nest = |levels: usize| {
            (1..levels).fold(
                json!({"type": 99}),
                |inner, _| json!({"type": 99, "components": [inner]}),
            )
        };
        assert_eq!(count(nest(32)), 32);
        let v = violations(nest(33));
        let path = vec!["components[0]"; 32].join(".");
        assert_eq!(summary(&v), [(RuleId::CV2_16, path.as_str())]);
        assert_eq!(
            (v[0].limit.clone(), v[0].actual.clone()),
            (Bound::AtMost(32), Actual::Count(33))
        );
    }

    #[test]
    fn cv2_16_raw_every_violation_in_pre_order() {
        let v = violations(json!({"type": 99, "id": 1, "components": [1, {"type": 0}]}));
        assert_eq!(
            summary(&v),
            [
                (RuleId::CV2_14, "id"),
                (RuleId::CV2_16, "components[0]"),
                (RuleId::CV2_16, "components[1]"),
            ]
        );
    }

    #[test]
    fn cv2_16_raw_is_verbatim() {
        let input = json!({
            "type": 99,
            "content": "😀ü",
            "foo": {"bar": [1, 2]},
            "components": [{"type": 98, "x": null}],
        });
        let raw = RawComponent::new(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(&raw).unwrap(), input);
        assert_eq!(raw.as_json(), &input);
        assert_eq!(raw.into_json(), input);
    }

    #[test]
    fn cv2_14_raw_top_level_ids_stay_consecutive() {
        let raw = json!({"type": 99, "components": [{"type": 98}]});
        let body = Components::new()
            .push(TextDisplay::new("a"))
            .push(RawComponent::new(raw.clone()).unwrap())
            .push(TextDisplay::new("b"));
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!([
                {"type": 10, "id": 1, "content": "a"},
                raw,
                {"type": 10, "id": 2, "content": "b"}
            ])
        );
    }

    #[test]
    fn cv2_14_raw_in_container_has_no_id() {
        let raw = json!({"type": 99});
        let body = Components::new().push(
            Container::new()
                .push(TextDisplay::new("a"))
                .push(RawComponent::new(raw.clone()).unwrap())
                .push(TextDisplay::new("b")),
        );
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!([{"type": 17, "id": 1, "components": [
                {"type": 10, "id": 2, "content": "a"},
                raw,
                {"type": 10, "id": 3, "content": "b"}
            ]}])
        );
    }

    #[test]
    fn cv2_16_raw_attachment_urls_in_document_order() {
        let raw = RawComponent::new(json!({
            "type": 99,
            "a": "attachment://1.png",
            "b": [{"deep": ["x", "attachment://2.png"]}],
            "c": "https://attachment://no",
        }))
        .unwrap();
        assert_eq!(
            raw.attachment_urls(),
            ["attachment://1.png", "attachment://2.png"]
        );
    }

    #[test]
    fn cv2_16_unknown_components_pre_order() {
        let message = [
            json!({"type": 10, "content": "a"}),
            json!({"type": 99, "x": 1}),
            json!({"type": 17, "components": [{"type": 98}]}),
            json!({"type": 9, "components": [{"type": 10}], "accessory": {"type": 97}}),
        ];
        assert_eq!(
            unknown_components(&message),
            [
                json!({"type": 99, "x": 1}),
                json!({"type": 98}),
                json!({"type": 97})
            ]
        );
    }

    #[test]
    fn cv2_16_unknown_container_taken_whole() {
        let node = json!({"type": 99, "components": [{"type": 96}]});
        assert_eq!(unknown_components(core::slice::from_ref(&node)), [node]);
    }

    #[test]
    fn cv2_16_unknown_odd_elements() {
        let odd = [json!(1), json!({}), json!({"type": "x"})];
        assert_eq!(unknown_components(&odd), odd);
        // Wrong shapes inside known nodes are skipped.
        let shapes = [
            json!({"type": 17, "components": {}}),
            json!({"type": 9, "components": [], "accessory": [1]}),
        ];
        assert!(unknown_components(&shapes).is_empty());
    }

    #[test]
    fn cv2_16_unknown_none() {
        let known: Vec<Value> = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17]
            .into_iter()
            .map(|t| json!({"type": t}))
            .collect();
        assert!(unknown_components(&known).is_empty());
        assert!(unknown_components(&[]).is_empty());
    }

    #[test]
    fn cv2_16_unknown_in_legacy_rows() {
        let rows = [json!({"type": 1, "components": [{"type": 2}, {"type": 95}]})];
        assert_eq!(unknown_components(&rows), [json!({"type": 95})]);
    }
}
