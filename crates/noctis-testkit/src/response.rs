//! What a run answered: [`Response`], the messages it sent
//! ([`TestMessage`]), their components ([`TestComponent`]), an opened
//! modal ([`ModalForm`]), and [`ResponseKind`], [`WireCost`], [`HttpCall`].

use std::sync::Arc;

use noctis_ui::{ButtonStyle, Handled};
use serde_json::Value;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};

use crate::TestError;
use crate::fake::RecordedFile;
use crate::render::mask;

/// `IS_COMPONENTS_V2`.
pub(crate) const V2_FLAG: u64 = 1 << 15;
/// `EPHEMERAL`.
pub(crate) const EPHEMERAL_FLAG: u64 = 1 << 6;

/// How a run answered, with the defer equivalences folded in: a type-5
/// defer followed by an edit of `@original` is a [`Reply`](Self::Reply),
/// a type-6 defer followed by an edit is an [`Update`](Self::Update).
///
/// ```
/// use noctis_testkit::ResponseKind;
///
/// assert_ne!(ResponseKind::Reply, ResponseKind::Deferred);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResponseKind {
    /// A new message: callback type 4, or 5 then an edit of `@original`.
    Reply,
    /// The source message edited: callback type 7, or 6 then an edit.
    Update,
    /// A type-5 or type-6 defer that was never edited (or whose original
    /// was deleted).
    Deferred,
    /// A modal: callback type 9.
    Modal,
    /// Autocomplete suggestions: callback type 8.
    Autocomplete,
    /// No callback at all (a Middleware stopped the run silently).
    None,
}

/// What a message costs against the message Limits, counted on the JSON as
/// sent: components at every depth (CV2-03) and Text Display characters
/// (CV2-04; `content` for a legacy message), in Discord's length unit.
///
/// It equals the two numbers of `noctis::ui::cost` for the same message.
///
/// ```
/// use noctis_testkit::WireCost;
///
/// assert_eq!(WireCost::default(), WireCost { components: 0, text_chars: 0 });
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WireCost {
    /// Components at every depth.
    pub components: usize,
    /// Text Display (or `content`) characters.
    pub text_chars: usize,
}

/// One HTTP request the App sent during a run.
#[derive(Debug, Clone)]
pub struct HttpCall {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) json: Option<Value>,
    pub(crate) files: Vec<RecordedFile>,
}

impl HttpCall {
    /// The method, e.g. `POST`.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The path after `/api/v10`, e.g. `/interactions/1/t/callback`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The JSON body, when there is one; the `payload_json` part of a
    /// multipart request (a message with uploads).
    pub fn json(&self) -> Option<&Value> {
        self.json.as_ref()
    }

    /// The uploaded files (`files[n]` parts) of a multipart request, in
    /// body order; empty for a JSON request.
    pub fn files(&self) -> &[RecordedFile] {
        &self.files
    }
}

/// Who ran an interaction, where, in which locale.
#[derive(Debug, Clone)]
pub(crate) struct RunCtx {
    pub(crate) user: Id<UserMarker>,
    pub(crate) guild: Option<Id<GuildMarker>>,
    pub(crate) channel: Id<ChannelMarker>,
    pub(crate) channel_type: u8,
    pub(crate) locale: String,
    pub(crate) guild_locale: Option<String>,
}

/// A message as a later interaction on it sees it.
#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) id: Id<MessageMarker>,
    /// The complete message object (`message` of a component interaction).
    pub(crate) json: Value,
    pub(crate) ephemeral: bool,
    /// The run that produced it: the default user, guild and channel of a
    /// click on it.
    pub(crate) ctx: RunCtx,
}

/// A message the App sent (or edited) in a run.
///
/// ```no_run
/// # async fn f(app: &noctis_testkit::TestApp) -> Result<(), noctis_testkit::TestError> {
/// let response = app.command("ping").run().await?;
/// if let Some(msg) = response.message() {
///     println!("{} {}", msg.id(), msg.text());
/// }
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct TestMessage {
    pub(crate) raw: Value,
    pub(crate) source: Arc<Source>,
}

impl TestMessage {
    /// The message id (the testkit assigns one to every reply and
    /// follow-up; an update keeps its message's id).
    pub fn id(&self) -> Id<MessageMarker> {
        self.source.id
    }

    /// The body exactly as sent, unmasked.
    pub fn raw_json(&self) -> &Value {
        &self.raw
    }

    /// The body as sent with every `custom_id`'s per-send parts masked
    /// (select nonce `NN`, Ref `~RRRRRRRRRRR`, tag `TTTTTT`): stable for
    /// snapshots.
    pub fn message_json(&self) -> Value {
        mask(&self.raw)
    }

    /// Whether the message is ephemeral.
    pub fn is_ephemeral(&self) -> bool {
        self.source.ephemeral
    }

    /// Every Text Display's content in pre-order, joined by `\n` (the
    /// `content` of a legacy message).
    pub fn text(&self) -> String {
        if is_v2(&self.raw) {
            let mut texts = Vec::new();
            for node in nodes(&self.raw) {
                if kind_of(node) == Some(10)
                    && let Some(content) = node.get("content").and_then(Value::as_str)
                {
                    texts.push(content);
                }
            }
            texts.join("\n")
        } else {
            self.raw
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        }
    }

    /// The message's [`WireCost`].
    pub fn cost(&self) -> WireCost {
        let all = nodes(&self.raw);
        let text_chars = if is_v2(&self.raw) {
            all.iter()
                .filter(|node| kind_of(node) == Some(10))
                .filter_map(|node| node.get("content").and_then(Value::as_str))
                .map(noctis_rules::len::discord_len)
                .sum()
        } else {
            self.raw
                .get("content")
                .and_then(Value::as_str)
                .map_or(0, noctis_rules::len::discord_len)
        };
        WireCost {
            components: all.len(),
            text_chars,
        }
    }

    /// Every component at every depth, in pre-order (a Section's texts,
    /// then its accessory).
    pub fn components(&self) -> Vec<TestComponent> {
        nodes(&self.raw)
            .into_iter()
            .map(|json| TestComponent {
                json: json.clone(),
                source: Arc::clone(&self.source),
            })
            .collect()
    }

    /// Every button (link and premium buttons included), in pre-order.
    pub fn buttons(&self) -> Vec<TestComponent> {
        self.components()
            .into_iter()
            .filter(|c| c.component_type() == 2)
            .collect()
    }

    /// The first button labelled `label`.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] listing the labels there are.
    pub fn button(&self, label: &str) -> Result<TestComponent, TestError> {
        let buttons = self.buttons();
        let available = buttons
            .iter()
            .filter_map(|b| b.label().map(str::to_owned))
            .collect();
        buttons
            .into_iter()
            .find(|b| b.label() == Some(label))
            .ok_or_else(|| TestError::NotFound {
                what: format!("button {label:?}"),
                available,
            })
    }

    /// The first button whose emoji is `emoji` (a Unicode emoji, or a
    /// custom emoji's name).
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] listing the emojis there are.
    pub fn button_by_emoji(&self, emoji: &str) -> Result<TestComponent, TestError> {
        let buttons = self.buttons();
        let available = buttons
            .iter()
            .filter_map(|b| b.emoji().map(str::to_owned))
            .collect();
        buttons
            .into_iter()
            .find(|b| b.emoji() == Some(emoji))
            .ok_or_else(|| TestError::NotFound {
                what: format!("button with emoji {emoji:?}"),
                available,
            })
    }

    /// The first component built from Route `R` (a component struct).
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] listing the Routes there are.
    pub fn component<R: Handled>(&self) -> Result<TestComponent, TestError> {
        let route = R::ID.as_str();
        let components = self.components();
        let available = components
            .iter()
            .filter_map(|c| c.route().map(str::to_owned))
            .collect();
        components
            .into_iter()
            .find(|c| c.route() == Some(route))
            .ok_or_else(|| TestError::NotFound {
                what: format!("component of Route `{route}`"),
                available,
            })
    }
}

/// One component of a sent message: inspect it, or pass it to
/// [`TestApp::click`](crate::TestApp::click) /
/// [`TestApp::select`](crate::TestApp::select).
#[derive(Debug, Clone)]
pub struct TestComponent {
    pub(crate) json: Value,
    pub(crate) source: Arc<Source>,
}

impl TestComponent {
    /// The Discord component type (2 button, 3 string select, 10 Text
    /// Display, …).
    pub fn component_type(&self) -> u8 {
        kind_of(&self.json).unwrap_or(0)
    }

    /// The `custom_id` as sent (unmasked).
    pub fn custom_id(&self) -> Option<&str> {
        self.json.get("custom_id").and_then(Value::as_str)
    }

    /// The Route id of the `custom_id`, when it is a Noctis one.
    pub fn route(&self) -> Option<&str> {
        self.custom_id().and_then(noctis_core::__testing::route_of)
    }

    /// The label.
    pub fn label(&self) -> Option<&str> {
        self.json.get("label").and_then(Value::as_str)
    }

    /// The emoji: a Unicode emoji, or a custom emoji's name.
    pub fn emoji(&self) -> Option<&str> {
        self.json
            .get("emoji")
            .and_then(|e| e.get("name"))
            .and_then(Value::as_str)
    }

    /// The style of an interactive button; `None` for link and premium
    /// buttons and for other components.
    pub fn style(&self) -> Option<ButtonStyle> {
        if self.component_type() != 2 {
            return None;
        }
        match self.json.get("style").and_then(Value::as_u64) {
            Some(1) => Some(ButtonStyle::Primary),
            Some(2) => Some(ButtonStyle::Secondary),
            Some(3) => Some(ButtonStyle::Success),
            Some(4) => Some(ButtonStyle::Danger),
            _ => None,
        }
    }

    /// Whether it is disabled.
    pub fn is_disabled(&self) -> bool {
        self.json
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// A string select's option values, in order.
    pub fn options(&self) -> Vec<String> {
        self.json
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|o| o.get("value").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The kind name used in errors.
    pub(crate) fn kind_name(&self) -> &'static str {
        kind_name(self.component_type())
    }
}

/// The name of component type `kind`.
pub(crate) const fn kind_name(kind: u8) -> &'static str {
    match kind {
        2 => "button",
        3 => "string select",
        5 => "user select",
        6 => "role select",
        7 => "mentionable select",
        8 => "channel select",
        _ => "non-interactive component",
    }
}

/// One setter of a [`ModalForm`].
#[derive(Debug, Clone)]
pub(crate) enum Setter {
    Text(String),
    Values(Vec<String>),
    Radio(String),
    Checkbox(bool),
}

/// A modal the App opened, filled in by the test and submitted with
/// [`TestApp::submit`](crate::TestApp::submit).
///
/// Unset inputs submit their pre-filled value (or nothing); setter
/// mistakes (an unknown key, the wrong kind) surface when the submit runs.
///
/// ```no_run
/// # async fn f(app: &noctis_testkit::TestApp) -> Result<(), noctis_testkit::TestError> {
/// let opened = app.command("feedback").run().await?;
/// let form = opened.modal()?.text("note", "hi").checkbox("agree", true);
/// let saved = app.submit(form).run().await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct ModalForm {
    pub(crate) data: Value,
    pub(crate) source: Option<Arc<Source>>,
    pub(crate) ctx: RunCtx,
    pub(crate) set: Vec<(String, Setter)>,
}

impl ModalForm {
    /// The modal's `custom_id`.
    pub fn custom_id(&self) -> &str {
        self.data
            .get("custom_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// The modal's title.
    pub fn title(&self) -> &str {
        self.data
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// Types `value` into the text input `key`.
    #[must_use]
    pub fn text(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set.push((key.into(), Setter::Text(value.into())));
        self
    }

    /// Selects `values` in the select or Checkbox Group `key` (ids as
    /// strings for user, role, mentionable and channel selects).
    #[must_use]
    pub fn values<I>(mut self, key: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let values = values.into_iter().map(Into::into).collect();
        self.set.push((key.into(), Setter::Values(values)));
        self
    }

    /// Picks `value` in the Radio Group `key`.
    #[must_use]
    pub fn radio(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.set.push((key.into(), Setter::Radio(value.into())));
        self
    }

    /// Ticks (or clears) the Checkbox `key`.
    #[must_use]
    pub fn checkbox(mut self, key: impl Into<String>, checked: bool) -> Self {
        self.set.push((key.into(), Setter::Checkbox(checked)));
        self
    }
}

/// What a run answered.
///
/// The message accessors (`text`, `cost`, `button`, …) read the final
/// message: the callback's, replaced by later edits of `@original`; they
/// give empty results (or [`TestError::NotFound`]) when there is none.
#[derive(Debug, Clone)]
pub struct Response {
    pub(crate) kind: ResponseKind,
    pub(crate) callback_type: Option<u8>,
    pub(crate) ephemeral: bool,
    pub(crate) message: Option<TestMessage>,
    pub(crate) followups: Vec<TestMessage>,
    pub(crate) modal: Option<ModalForm>,
    pub(crate) suggestions: Vec<(String, String)>,
    pub(crate) http_calls: Vec<HttpCall>,
    pub(crate) notices: Notices,
}

/// Which built-in notices a run showed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Notices {
    pub(crate) guard: bool,
    pub(crate) expired: bool,
    pub(crate) error: bool,
}

impl Response {
    /// How the run answered.
    pub fn kind(&self) -> ResponseKind {
        self.kind
    }

    /// The first callback's type, if any.
    pub fn callback_type(&self) -> Option<u8> {
        self.callback_type
    }

    /// Whether the answer is ephemeral (flag 64 of a type 4 or 5 callback;
    /// an update keeps its message's).
    pub fn is_ephemeral(&self) -> bool {
        self.ephemeral
    }

    /// The final message, if any.
    pub fn message(&self) -> Option<&TestMessage> {
        self.message.as_ref()
    }

    /// The follow-up messages, in order.
    pub fn followups(&self) -> &[TestMessage] {
        &self.followups
    }

    /// The modal the run opened.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`] when the run opened none.
    pub fn modal(&self) -> Result<ModalForm, TestError> {
        self.modal.clone().ok_or_else(|| TestError::NotFound {
            what: "modal".to_owned(),
            available: Vec::new(),
        })
    }

    /// The autocomplete suggestions as sent: `(value as a string, label)`.
    pub fn suggestions(&self) -> &[(String, String)] {
        &self.suggestions
    }

    /// Whether a Route Guard's built-in notice (`noctis.guard.failed`,
    /// `noctis.guard.owner`, `noctis.guard.permissions`) was shown.
    pub fn is_guard_notice(&self) -> bool {
        self.notices.guard
    }

    /// Whether the built-in `noctis.expired` notice was shown.
    pub fn is_expired_notice(&self) -> bool {
        self.notices.expired
    }

    /// Whether the built-in error notice (`noctis.error.generic` or
    /// `noctis.validation.failed`) was shown.
    pub fn is_error(&self) -> bool {
        self.notices.error
    }

    /// Every HTTP request of the run, in order.
    pub fn http_calls(&self) -> &[HttpCall] {
        &self.http_calls
    }

    /// The final message's text (`""` without one).
    pub fn text(&self) -> String {
        self.message
            .as_ref()
            .map(TestMessage::text)
            .unwrap_or_default()
    }

    /// The final message's masked JSON (`Null` without one).
    pub fn message_json(&self) -> Value {
        self.message
            .as_ref()
            .map_or(Value::Null, TestMessage::message_json)
    }

    /// The final message's [`WireCost`] (zero without one).
    pub fn cost(&self) -> WireCost {
        self.message
            .as_ref()
            .map(TestMessage::cost)
            .unwrap_or_default()
    }

    /// The final message's components.
    pub fn components(&self) -> Vec<TestComponent> {
        self.message
            .as_ref()
            .map(TestMessage::components)
            .unwrap_or_default()
    }

    /// The final message's buttons.
    pub fn buttons(&self) -> Vec<TestComponent> {
        self.message
            .as_ref()
            .map(TestMessage::buttons)
            .unwrap_or_default()
    }

    /// The final message's first button labelled `label`.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`].
    pub fn button(&self, label: &str) -> Result<TestComponent, TestError> {
        match &self.message {
            Some(message) => message.button(label),
            None => Err(no_message(format!("button {label:?}"))),
        }
    }

    /// The final message's first button with `emoji`.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`].
    pub fn button_by_emoji(&self, emoji: &str) -> Result<TestComponent, TestError> {
        match &self.message {
            Some(message) => message.button_by_emoji(emoji),
            None => Err(no_message(format!("button with emoji {emoji:?}"))),
        }
    }

    /// The final message's first component built from Route `R`.
    ///
    /// # Errors
    ///
    /// [`TestError::NotFound`].
    pub fn component<R: Handled>(&self) -> Result<TestComponent, TestError> {
        match &self.message {
            Some(message) => message.component::<R>(),
            None => Err(no_message(format!(
                "component of Route `{}`",
                R::ID.as_str()
            ))),
        }
    }
}

fn no_message(what: String) -> TestError {
    TestError::NotFound {
        what,
        available: Vec::new(),
    }
}

/// Whether a body is a Components V2 one.
pub(crate) fn is_v2(body: &Value) -> bool {
    body.get("flags")
        .and_then(Value::as_u64)
        .is_some_and(|f| f & V2_FLAG != 0)
}

/// The numeric `type` of a component object.
pub(crate) fn kind_of(node: &Value) -> Option<u8> {
    node.get("type")
        .and_then(Value::as_u64)
        .and_then(|t| u8::try_from(t).ok())
}

/// Every component of `body` at every depth, in pre-order: the objects
/// with a numeric `type` in `components` arrays and `accessory` slots
/// (select options, gallery items and thumbnail media are not components).
pub(crate) fn nodes(body: &Value) -> Vec<&Value> {
    fn walk<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
        if let Some(children) = node.get("components").and_then(Value::as_array) {
            for child in children.iter().filter(|c| kind_of(c).is_some()) {
                out.push(child);
                walk(child, out);
            }
        }
        if let Some(accessory) = node.get("accessory").filter(|a| kind_of(a).is_some()) {
            out.push(accessory);
            walk(accessory, out);
        }
    }
    let mut out = Vec::new();
    walk(body, &mut out);
    out
}

/// The content of a body that is exactly one Text Display.
pub(crate) fn single_text(body: &Value) -> Option<&str> {
    let [only] = body.get("components")?.as_array()?.as_slice() else {
        return None;
    };
    (kind_of(only) == Some(10))
        .then(|| only.get("content").and_then(Value::as_str))
        .flatten()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

    use serde_json::json;

    use super::*;

    fn ctx() -> RunCtx {
        RunCtx {
            user: Id::new(1),
            guild: None,
            channel: Id::new(1),
            channel_type: 1,
            locale: "en-US".to_owned(),
            guild_locale: None,
        }
    }

    fn message(raw: Value) -> TestMessage {
        TestMessage {
            raw,
            source: Arc::new(Source {
                id: Id::new(5),
                json: Value::Null,
                ephemeral: false,
                ctx: ctx(),
            }),
        }
    }

    fn button(label: &str, style: u8, disabled: bool) -> Value {
        json!({ "type": 2, "style": style, "label": label, "custom_id": format!("0b:{label}"), "disabled": disabled })
    }

    /// M1/M6: the pre-order walk counts a Section's texts and accessory.
    fn panel() -> Value {
        json!({
            "flags": 32768,
            "components": [{
                "type": 17, "components": [
                    { "type": 10, "content": "## Title" },
                    { "type": 9, "components": [{ "type": 10, "content": "a" }],
                      "accessory": button("Go", 1, false) },
                    { "type": 1, "components": [
                        button("Next", 1, false),
                        button("Prev", 2, true),
                        { "type": 2, "style": 5, "label": "Docs", "url": "https://a.b" },
                    ]},
                ],
            }],
        })
    }

    #[test]
    fn cost_and_text_of_a_v2_panel() {
        let msg = message(panel());
        assert_eq!(msg.text(), "## Title\na");
        assert_eq!(
            msg.cost(),
            WireCost {
                components: 9,
                text_chars: 9
            }
        );
        let labels: Vec<_> = msg
            .buttons()
            .iter()
            .map(|b| b.label().unwrap().to_owned())
            .collect();
        assert_eq!(labels, ["Go", "Next", "Prev", "Docs"]);
        assert!(msg.button("Prev").unwrap().is_disabled());
        assert_eq!(
            msg.button("Next").unwrap().style(),
            Some(ButtonStyle::Primary)
        );
        assert_eq!(msg.button("Docs").unwrap().style(), None);
        match msg.button("Missing") {
            Err(TestError::NotFound { what, available }) => {
                assert_eq!(what, "button \"Missing\"");
                assert_eq!(available, ["Go", "Next", "Prev", "Docs"]);
            }
            other => panic!("{other:?}"),
        }
    }

    /// M7: surrogate pairs count twice; select options and gallery items
    /// are not components.
    #[test]
    fn cost_counts_discord_units_and_skips_non_components() {
        let msg = message(json!({ "flags": 32768, "components": [
            { "type": 10, "content": "😀é" },
        ]}));
        assert_eq!(msg.cost().text_chars, 3);
        let options: Vec<Value> = (0..25)
            .map(|i| json!({ "label": i.to_string(), "value": i.to_string() }))
            .collect();
        let msg = message(json!({ "flags": 32768, "components": [
            { "type": 1, "components": [{ "type": 3, "custom_id": "0s:", "options": options }] },
        ]}));
        assert_eq!(msg.cost().components, 2);
        assert_eq!(msg.components()[1].options().len(), 25);
        let items: Vec<Value> = (0..10)
            .map(|_| json!({ "media": { "url": "https://a.b/c.png" } }))
            .collect();
        let msg = message(json!({ "flags": 32768, "components": [
            { "type": 12, "items": items },
        ]}));
        assert_eq!(msg.cost().components, 1);
    }

    /// M8: a legacy message counts rows, their items and `content`.
    #[test]
    fn cost_of_a_legacy_message() {
        let msg = message(json!({ "content": "hi", "components": [
            { "type": 1, "components": [button("A", 1, false), button("B", 2, false)] },
        ]}));
        assert_eq!(
            msg.cost(),
            WireCost {
                components: 3,
                text_chars: 2
            }
        );
        assert_eq!(msg.text(), "hi");
    }

    /// M3/M4: the first match in pre-order; emoji by Unicode or name.
    #[test]
    fn finders_take_the_first_match() {
        let msg = message(json!({ "flags": 32768, "components": [
            { "type": 1, "components": [
                { "type": 2, "style": 1, "label": "Next", "custom_id": "0a:1" },
                { "type": 2, "style": 1, "label": "Next", "custom_id": "0a:2" },
                { "type": 2, "style": 2, "emoji": { "name": "▶" }, "custom_id": "0a:3" },
                { "type": 2, "style": 2, "emoji": { "name": "next", "id": "9" }, "custom_id": "0a:4" },
            ]},
        ]}));
        assert_eq!(msg.button("Next").unwrap().custom_id(), Some("0a:1"));
        assert_eq!(msg.button_by_emoji("▶").unwrap().custom_id(), Some("0a:3"));
        assert_eq!(
            msg.button_by_emoji("next").unwrap().custom_id(),
            Some("0a:4")
        );
        assert_eq!(msg.button("Next").unwrap().route(), Some("a"));
    }

    #[test]
    fn single_text_detects_notices() {
        assert_eq!(
            single_text(&json!({ "components": [{ "type": 10, "content": "x" }] })),
            Some("x")
        );
        assert_eq!(single_text(&json!({ "components": [] })), None);
        assert_eq!(single_text(&panel()), None);
    }
}
