//! Autocomplete (FR-10, AD-11; INT-06, INT-07, OPT-11, OPT-12): the
//! [`AutocompleteCtx`] suggestion methods receive, the [`Focused`] option,
//! the [`Suggestions`] they return, the typed [`Partial<O>`](Partial) of the
//! other options and the hidden binding `#[autocomplete(field)]` expands to.
//!
//! A suggestion method answers with its return value; there is no defer
//! and no Responder (INT-06). The pipeline lowers the [`Suggestions`] to the
//! one type-8 callback: more than 25 are truncated with a warning (INT-07),
//! a `MessageKey` label is resolved in the user's locale, a label over 100
//! characters is cut with `…` and a warning, an empty label or a value
//! Discord would reject is dropped with a warning, and an error, a panic or
//! a missed 2.5 s deadline answers an empty list and goes to the
//! Autocomplete Error Sink.

use std::fmt;
use std::future::Future;
#[cfg(test)]
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use noctis_i18n::Builtin;
use noctis_rules::command::{Choice, ChoiceValue, OptionType, validate_autocomplete};
use noctis_rules::{Limits, MessageKey, RuleId, Translate};
use serde_json::{Map, Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::options::{
    CommandInput, OptionError, OptionValue, Options, decode_interaction, decode_value, option_name,
    option_type,
};

/// How long an autocomplete handler may run, measured from when the
/// interaction arrived; then the pipeline answers an empty list. Discord
/// allows 3 s, and the rest is left for the callback.
pub(crate) const AUTOCOMPLETE_DEADLINE: Duration = Duration::from_millis(2500);

/// The option the user is typing in: its name, its type and the raw text
/// typed so far.
///
/// ```
/// # use noctis_core::{AutocompleteCtx, HandlerResult, Suggestions};
/// fn suggest(ctx: &AutocompleteCtx) -> HandlerResult<Suggestions<String>> {
///     let typed = ctx.focused().value().to_owned();
///     let limit = ctx.focused().as_integer().unwrap_or(0);
///     Ok([(typed.clone(), format!("{typed} ({limit})"))].into_iter().collect())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Focused {
    name: String,
    kind: OptionType,
    value: String,
}

impl Focused {
    /// The focused option's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The focused option's type (STRING, INTEGER or NUMBER for a declared
    /// autocomplete option).
    pub fn kind(&self) -> OptionType {
        self.kind
    }

    /// The text typed so far, exactly as sent (a JSON number is kept as its
    /// text). It is unvalidated: an INTEGER option may hold `"-"` here.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The typed text as an `i64`, when it parses as one.
    pub fn as_integer(&self) -> Option<i64> {
        self.value.parse().ok()
    }

    /// The typed text as a finite `f64`, when it parses as one (`NaN` and
    /// `inf` are `None`).
    pub fn as_number(&self) -> Option<f64> {
        self.value.parse::<f64>().ok().filter(|v| v.is_finite())
    }
}

/// The value of an [`AutocompleteChoice`]: a STRING, INTEGER or NUMBER
/// value, matching the focused option's type; what a [`SuggestionValue`]
/// lowers to.
///
/// ```
/// use noctis_core::AutocompleteValue;
///
/// assert_eq!(AutocompleteValue::from("a"), AutocompleteValue::String("a".into()));
/// assert_eq!(AutocompleteValue::from(3_i64), AutocompleteValue::Integer(3));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AutocompleteValue {
    /// A STRING value (at most 100 characters).
    String(String),
    /// An INTEGER value (also fits a NUMBER option).
    Integer(i64),
    /// A NUMBER value (finite, within ±2^53).
    Number(f64),
}

impl From<&str> for AutocompleteValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for AutocompleteValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<i64> for AutocompleteValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for AutocompleteValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl AutocompleteValue {
    fn rules(&self) -> ChoiceValue<'_> {
        match self {
            Self::String(v) => ChoiceValue::Str(v),
            Self::Integer(v) => ChoiceValue::Int(*v),
            Self::Number(v) => ChoiceValue::Float(*v),
        }
    }

    fn json(&self) -> Value {
        match self {
            Self::String(v) => json!(v),
            Self::Integer(v) => json!(v),
            Self::Number(v) => json!(v),
        }
    }
}

/// One autocomplete choice as sent: the name shown (1 to 100 characters)
/// and the value the option takes when it is picked; what one of the
/// [`Suggestions`] lowers to.
///
/// ```
/// use noctis_core::{AutocompleteChoice, AutocompleteValue};
///
/// let choice = AutocompleteChoice::new("Prefix !", "!");
/// assert_eq!(choice.name(), "Prefix !");
/// assert_eq!(choice.value(), &AutocompleteValue::String("!".into()));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AutocompleteChoice {
    name: String,
    value: AutocompleteValue,
}

impl AutocompleteChoice {
    /// A choice named `name` with `value`.
    pub fn new(name: impl Into<String>, value: impl Into<AutocompleteValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// The displayed name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The value.
    pub fn value(&self) -> &AutocompleteValue {
        &self.value
    }
}

/// The parts of an [`AutocompleteCtx`].
pub(crate) struct AutocompleteParts {
    pub(crate) path: &'static str,
    pub(crate) module: &'static str,
    pub(crate) shard: u32,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    pub(crate) user_id: Id<UserMarker>,
    /// The run's resolved locales (stage 4 and the App's
    /// `LocaleResolver`; autocomplete runs no Middleware).
    pub(crate) locales: Arc<crate::locale::LocaleCell>,
    pub(crate) focused: Focused,
    /// `None` in unit tests and the detached seam.
    pub(crate) app: Option<Arc<AppShared>>,
}

/// The context an `#[autocomplete(field)]` method receives: the Command
/// Path, the Module, the shard, the guild, channel and user, the locales,
/// the [`Focused`] option, the App's typed state and its HTTP client.
///
/// There is no defer and no Responder (INT-06): the method returns its
/// [`Suggestions`] and Noctis sends them.
///
/// ```compile_fail
/// use noctis_core::{AutocompleteCtx, HandlerResult, Suggestions};
///
/// async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Suggestions<String>> {
///     ctx.defer_reply(); // [INT-06]
///     Ok(Suggestions::new())
/// }
/// ```
pub struct AutocompleteCtx {
    parts: AutocompleteParts,
}

impl AutocompleteCtx {
    /// A context of `parts`.
    pub(crate) fn new(parts: AutocompleteParts) -> Self {
        Self { parts }
    }

    /// The App's name (the `app` metric label), when there is an App.
    pub(crate) fn app_name(&self) -> Option<Arc<str>> {
        self.parts.app.as_ref().map(|app| Arc::clone(&app.app))
    }

    /// How labels given as a `MessageKey` render: the App's strings in the
    /// run's resolved User Locale.
    pub(crate) fn labels(&self) -> Labels {
        Labels {
            strings: self.parts.app.as_ref().map(|app| Arc::clone(&app.strings)),
            locales: Arc::clone(&self.parts.locales),
        }
    }

    /// The Command Path whose option is autocompleted, words joined by one
    /// ASCII space (`"config set prefix"`).
    pub fn command_path(&self) -> &'static str {
        self.parts.path
    }

    /// The id of the Module the Command was added to.
    pub fn module(&self) -> &'static str {
        self.parts.module
    }

    /// The number of the shard that received the interaction.
    pub fn shard(&self) -> u32 {
        self.parts.shard
    }

    /// The guild the Command is typed in.
    ///
    /// # Errors
    ///
    /// [`AutocompleteError::NotInGuild`] in a DM or a group DM, so
    /// `ctx.guild_id()?` answers an empty list there.
    pub fn guild_id(&self) -> Result<Id<GuildMarker>, HandlerError> {
        self.parts
            .guild_id
            .ok_or_else(|| HandlerError::new(AutocompleteError::NotInGuild))
    }

    /// The channel the Command is typed in, if Discord sent it.
    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.parts.channel_id
    }

    /// The typing user.
    pub fn user_id(&self) -> Id<UserMarker> {
        self.parts.user_id
    }

    /// The resolved User Locale (FR-43): Discord's `locale`, then the
    /// Guild Locale, then the App's default, each step taking the code's
    /// own locale file or its sibling's; then the App's `LocaleResolver`.
    /// `MessageKey` labels render in it.
    pub fn user_locale(&self) -> noctis_rules::Locale {
        self.parts.locales.get().user
    }

    /// The resolved Guild Locale (FR-43): Discord's `guild_locale`, then
    /// (in a DM) the User Locale, then the App's default; then the App's
    /// `LocaleResolver`.
    pub fn guild_locale(&self) -> noctis_rules::Locale {
        self.parts.locales.get().guild
    }

    /// The option being typed in.
    pub fn focused(&self) -> &Focused {
        &self.parts.focused
    }

    /// The state registered for `T` with `AppBuilder::state`.
    ///
    /// # Panics
    ///
    /// When no `T` is registered (or outside an App); the pipeline catches
    /// the panic like any handler panic and answers an empty list.
    pub fn state<T: Send + Sync + 'static>(&self) -> State<T> {
        match self
            .parts
            .app
            .as_ref()
            .and_then(|app| app.states.get::<T>())
        {
            Some(state) => state,
            None => panic!(
                "no state of type `{}` is registered with `AppBuilder::state`",
                std::any::type_name::<T>()
            ),
        }
    }

    /// The App's typed key-value handle over its namespaced `CacheStore`
    /// (a cheap clone).
    pub fn kv(&self) -> crate::Kv {
        crate::Kv::of(self.parts.app.as_ref())
    }

    /// The App's one Discord HTTP client (the `Arc` `App::http` returns).
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        match &self.parts.app {
            Some(app) => &app.http,
            None => crate::command::detached_http(),
        }
    }
}

impl fmt::Debug for AutocompleteCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = &self.parts;
        f.debug_struct("AutocompleteCtx")
            .field("command_path", &p.path)
            .field("module", &p.module)
            .field("shard", &p.shard)
            .field("guild_id", &p.guild_id)
            .field("channel_id", &p.channel_id)
            .field("user_id", &p.user_id)
            .field("user_locale", &p.locales.get().user)
            .field("guild_locale", &p.locales.get().guild)
            .field("focused", &p.focused.name)
            .finish_non_exhaustive()
    }
}

/// How `MessageKey` labels render for one interaction.
pub(crate) struct Labels {
    strings: Option<Arc<dyn Translate>>,
    locales: Arc<crate::locale::LocaleCell>,
}

impl Labels {
    /// The text of `label`.
    fn text(&self, label: SuggestionLabel) -> String {
        match label.0 {
            LabelKind::Text(text) => text,
            LabelKind::Key(key) => {
                let locale = self.locales.get().user;
                match &self.strings {
                    Some(strings) => crate::notice::text(&**strings, locale, key),
                    None => crate::notice::text(&Builtin, locale, key),
                }
            }
        }
    }
}

/// The label of one suggestion: text, or a [`MessageKey`] resolved in the
/// user's locale (LOC-04).
///
/// ```
/// use noctis_core::SuggestionLabel;
/// use noctis_rules::MessageKey;
///
/// let _: SuggestionLabel = "Red".into();
/// let _: SuggestionLabel = String::from("Blue").into();
/// let _: SuggestionLabel = MessageKey::new("colour.green").into();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionLabel(LabelKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum LabelKind {
    Text(String),
    Key(MessageKey),
}

impl From<String> for SuggestionLabel {
    fn from(text: String) -> Self {
        Self(LabelKind::Text(text))
    }
}

impl From<&str> for SuggestionLabel {
    fn from(text: &str) -> Self {
        Self(LabelKind::Text(text.to_owned()))
    }
}

impl From<MessageKey> for SuggestionLabel {
    fn from(key: MessageKey) -> Self {
        Self(LabelKind::Key(key))
    }
}

mod sealed {
    pub trait Value {}
    impl Value for i64 {}
    impl Value for f64 {}
    impl Value for String {}
}

/// The value type of [`Suggestions`]: `i64` (INTEGER), `f64` (NUMBER) or
/// `String` (STRING), the autocomplete field's type without `Option`.
/// Sealed.
#[diagnostic::on_unimplemented(
    message = "[OPT-11] autocomplete suggestions are `i64`, `f64` or `String`",
    label = "not `i64`, `f64` or `String`",
    note = "autocomplete applies only to STRING, INTEGER and NUMBER options"
)]
pub trait SuggestionValue: sealed::Value + Send + 'static {
    /// The wire value.
    #[doc(hidden)]
    fn __into_value(self) -> AutocompleteValue;
}

impl SuggestionValue for i64 {
    fn __into_value(self) -> AutocompleteValue {
        AutocompleteValue::Integer(self)
    }
}

impl SuggestionValue for f64 {
    fn __into_value(self) -> AutocompleteValue {
        AutocompleteValue::Number(self)
    }
}

impl SuggestionValue for String {
    fn __into_value(self) -> AutocompleteValue {
        AutocompleteValue::String(self)
    }
}

/// What an `#[autocomplete(field)]` method returns: values of the field's
/// type, each with the label the user sees, in order.
///
/// Noctis sends the first 25 (INT-07, with a warning), cuts a label over
/// 100 characters with `…` (with a warning) and drops an empty label or a
/// value Discord would reject (a `String` over 100 characters, an `i64`
/// outside ±2^53, a non-finite `f64`), with one warning per response.
///
/// ```
/// use noctis_core::Suggestions;
///
/// let mut rules: Suggestions<i64> = [(1, "No spam"), (2, "Be kind")].into_iter().collect();
/// rules.push(3, String::from("Stay on topic"));
/// assert_eq!(rules.len(), 3);
/// assert!(Suggestions::<String>::new().is_empty());
/// ```
pub struct Suggestions<T: SuggestionValue> {
    items: Vec<(T, SuggestionLabel)>,
}

impl<T: SuggestionValue> Suggestions<T> {
    /// No suggestions.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Appends `value`, shown as `label`.
    pub fn push(&mut self, value: T, label: impl Into<SuggestionLabel>) {
        self.items.push((value, label.into()));
    }

    /// How many suggestions there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there is none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The wire values and labels.
    fn lower(self) -> Raw {
        self.items
            .into_iter()
            .map(|(value, label)| (value.__into_value(), label))
            .collect()
    }
}

impl<T: SuggestionValue> Default for Suggestions<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SuggestionValue + Clone> Clone for Suggestions<T> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
        }
    }
}

impl<T: SuggestionValue + fmt::Debug> fmt::Debug for Suggestions<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.items).finish()
    }
}

impl<T: SuggestionValue, L: Into<SuggestionLabel>> FromIterator<(T, L)> for Suggestions<T> {
    fn from_iter<I: IntoIterator<Item = (T, L)>>(iter: I) -> Self {
        Self {
            items: iter
                .into_iter()
                .map(|(value, label)| (value, label.into()))
                .collect(),
        }
    }
}

impl<T: SuggestionValue, L: Into<SuggestionLabel>> Extend<(T, L)> for Suggestions<T> {
    fn extend<I: IntoIterator<Item = (T, L)>>(&mut self, iter: I) {
        self.items
            .extend(iter.into_iter().map(|(value, label)| (value, label.into())));
    }
}

/// What a suggestion method returned, before lowering.
pub(crate) type Raw = Vec<(AutocompleteValue, SuggestionLabel)>;

/// A method's return value as the suggestions of an option whose values are
/// `V`: only `HandlerResult<Suggestions<V>>` (OPT-11).
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[OPT-11] the `#[autocomplete]` method of a `{V}` option returns `Suggestions<{V}>`",
    label = "this method's return type",
    note = "return `HandlerResult<Suggestions<{V}>>`: `{V}` is the field's type without `Option`"
)]
pub trait __SuggestionsFor<V>: Send + 'static {
    /// The wire values and labels.
    #[doc(hidden)]
    fn __lower(self) -> HandlerResult<Raw>;
}

impl<V: SuggestionValue> __SuggestionsFor<V> for HandlerResult<Suggestions<V>> {
    fn __lower(self) -> HandlerResult<Raw> {
        self.map(Suggestions::lower)
    }
}

/// An option field flagged `autocomplete` of a `#[derive(SlashCommand)]`
/// struct, keyed by the FNV-1a hash of the field's name; `Value` is the
/// field's type without `Option`. Implemented by the derive.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[OPT-11] `{Self}` has no option flagged `autocomplete` for this method",
    label = "no flagged option of this name",
    note = "`#[autocomplete(field)]` names a field of `{Self}` that carries `#[option(.., autocomplete)]`"
)]
pub trait __AutocompleteField<const FIELD: u64>: Options {
    /// The field's type without `Option`.
    type Value;
}

/// A started suggestion method.
type AutocompleteFuture = BoxFuture<'static, HandlerResult<Raw>>;

/// The erased method call: decodes its partial input, then runs.
type ErasedCall = dyn Fn(AutocompleteCtx, &CommandInput) -> AutocompleteFuture + Send + Sync;

/// One `#[autocomplete(field)]` method with its command type erased; what a
/// `CommandHandler` looks up per focused option.
#[doc(hidden)]
#[derive(Clone)]
pub struct __AutocompleteMethod {
    name: &'static str,
    call: Arc<ErasedCall>,
}

impl __AutocompleteMethod {
    /// The method's path (`module_path!()::Struct::method`).
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    /// Starts the method for `ctx` over `input`.
    pub(crate) fn call(&self, ctx: AutocompleteCtx, input: &CommandInput) -> AutocompleteFuture {
        (self.call)(ctx, input)
    }

    /// The stand-in of a method that does not exist; the derive's
    /// missing-method check fails compilation before it can be used.
    #[doc(hidden)]
    pub fn __missing(name: &'static str) -> Self {
        let call = |_: AutocompleteCtx, _: &CommandInput| -> AutocompleteFuture {
            Box::pin(std::future::ready(Ok(Vec::new())))
        };
        Self {
            name,
            call: Arc::new(call),
        }
    }
}

impl fmt::Debug for __AutocompleteMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AutocompleteMethod")
            .field(&self.name)
            .finish()
    }
}

/// Binds the suggestion method `f` of the field hashed `FIELD` of the
/// command `S`; what `#[autocomplete(field)]` expands to. `f` gets the
/// typed text and the partial options.
#[doc(hidden)]
pub fn __autocomplete_method<S, const FIELD: u64, F, Fut, R>(
    name: &'static str,
    f: F,
) -> __AutocompleteMethod
where
    S: __AutocompleteField<FIELD>,
    F: Fn(AutocompleteCtx, String, Partial<S>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: __SuggestionsFor<<S as __AutocompleteField<FIELD>>::Value>,
{
    let call = move |ctx: AutocompleteCtx, input: &CommandInput| -> AutocompleteFuture {
        let typed = ctx.focused().value().to_owned();
        let filled = <Partial<S> as PartialOptions>::__from_input(input);
        let fut = f(ctx, typed, filled);
        Box::pin(async move { fut.await.__lower() })
    };
    __AutocompleteMethod {
        name,
        call: Arc::new(call),
    }
}

/// The erased method of the option `option` of `O`, if one is bound.
pub(crate) fn erased_for<O: Options>(option: &str) -> Option<__AutocompleteMethod> {
    O::__autocomplete(option)
}

/// What a method's suggestions lowered to: the first
/// `limits.autocomplete_choices` choices with their labels resolved and cut
/// to 100, how many were returned and how many labels were cut.
#[derive(Debug, Default)]
pub(crate) struct Collected {
    pub(crate) choices: Vec<AutocompleteChoice>,
    pub(crate) returned: usize,
    pub(crate) labels_cut: usize,
}

/// Lowers `raw`: keeps the first `max`, renders each label with `labels`
/// and cuts it to `limits.choice_name_len` (`noctis_rules::len::truncate`).
pub(crate) fn collect(raw: Raw, labels: &Labels, limits: &Limits) -> Collected {
    let returned = raw.len();
    let mut labels_cut = 0;
    let choices = raw
        .into_iter()
        .take(limits.autocomplete_choices)
        .map(|(value, label)| {
            let text = labels.text(label);
            let name = match noctis_rules::len::truncate(&text, NAME_LEN) {
                std::borrow::Cow::Borrowed(_) => text,
                std::borrow::Cow::Owned(cut) => {
                    labels_cut += 1;
                    cut
                }
            };
            AutocompleteChoice { name, value }
        })
        .collect();
    Collected {
        choices,
        returned,
        labels_cut,
    }
}

/// The longest choice name Discord takes (OPT-09).
const NAME_LEN: usize = 100;

/// Why an autocomplete handler produced no choices, besides its own error.
///
/// ```
/// use std::time::Duration;
/// use noctis_core::AutocompleteError;
///
/// let err = AutocompleteError::DeadlineMissed { deadline: Duration::from_millis(2500) };
/// assert_eq!(err.to_string(), "the autocomplete handler did not respond within 2.5s");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, thiserror::Error)]
pub enum AutocompleteError {
    /// The handler had not returned by `received_at + deadline` (or the
    /// deadline had passed before it could start); an empty list was sent.
    #[error("the autocomplete handler did not respond within {deadline:?}")]
    DeadlineMissed {
        /// The deadline, measured from when the interaction arrived.
        deadline: Duration,
    },
    /// [`AutocompleteCtx::guild_id`] outside a guild; the user gets an
    /// empty list.
    #[error("the autocomplete interaction is not in a guild")]
    NotInGuild,
}

/// The options of `Options` as `Option`s, what an autocomplete method gets
/// as `filled` (OPT-12); `#[derive(SlashCommand)]` generates it as
/// `<Struct>Partial`.
///
/// Decoding never fails: a missing, mistyped, unresolved or unknown-choice
/// value is `None`, and validators never run.
pub trait PartialOptions: Default + Send + 'static {
    /// The options struct this is the partial form of.
    type Options: Options;

    /// Decodes the present, well-typed values of `input`.
    #[doc(hidden)]
    fn __from_input(input: &CommandInput) -> Self;
}

/// A partial form with no fields, for hand-written test options.
#[cfg(test)]
pub(crate) struct NoPartial<O>(PhantomData<fn() -> O>);

#[cfg(test)]
impl<O> Default for NoPartial<O> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[cfg(test)]
impl<O: Options> PartialOptions for NoPartial<O> {
    type Options = O;

    fn __from_input(_: &CommandInput) -> Self {
        Self::default()
    }
}

/// The partial form of the options struct `O`: every field an `Option`
/// (OPT-12). An `#[autocomplete(field)]` method takes it as
/// `filled: Partial<Self>`.
///
/// ```compile_fail
/// use noctis_core::{CommandInput, Options, Partial, PartialOptions};
/// # use noctis_core::{CommandOption, OptionError};
/// # use noctis_rules::command::OptionDecl;
/// # #[derive(Default)]
/// # struct LimitPartial { limit: Option<i64> }
/// # impl PartialOptions for LimitPartial {
/// #     type Options = Limit;
/// #     fn __from_input(_: &CommandInput) -> Self { Self::default() }
/// # }
/// # struct Limit { limit: i64 }
/// # impl Options for Limit {
/// #     const OPTIONS: &'static [OptionDecl<'static>] = &[];
/// #     type Partial = LimitPartial;
/// #     fn from_input(_: &CommandInput) -> Result<Self, OptionError> { Ok(Self { limit: 0 }) }
/// # }
/// fn read(partial: Partial<Limit>) {
///     let limit: i64 = partial.limit; // [OPT-12]
/// }
/// ```
pub type Partial<O> = <O as Options>::Partial;

/// The decoded data of an autocomplete interaction.
pub(crate) struct AutocompleteInput;

impl AutocompleteInput {
    /// Decodes an `APPLICATION_COMMAND_AUTOCOMPLETE` interaction object:
    /// the partial input (only values that decode for their type; the
    /// focused value only when it parses for its type) and the focused
    /// option.
    ///
    /// # Errors
    ///
    /// [`OptionError::NotSlashCommand`] for another interaction or command
    /// type; [`OptionError::Malformed`] for malformed JSON or when not
    /// exactly one option is focused.
    pub(crate) fn from_json(raw: &[u8]) -> Result<(CommandInput, Focused), OptionError> {
        let (path, (options, focused), resolved) = decode_interaction(raw, 4, leaves)?;
        let mut input = CommandInput::from_parts(path, options, resolved);
        if let Some(value) = focused_value(&focused) {
            input = input.with_option(focused.name.clone(), value);
        }
        Ok((input, focused))
    }
}

/// The non-focused leaf values that decode, and the one focused option.
fn leaves(
    options: Vec<&Map<String, Value>>,
) -> Result<(Vec<(String, OptionValue)>, Focused), OptionError> {
    let mut out: Vec<(String, OptionValue)> = Vec::with_capacity(options.len());
    let mut focused = None;
    for option in options {
        if option.get("focused").and_then(Value::as_bool) == Some(true) {
            if focused.is_some() {
                return Err(OptionError::malformed("two options are focused"));
            }
            focused = Some(focus(option)?);
            continue;
        }
        let (Ok(name), Ok(kind), Some(value)) = (
            option_name(option),
            option_type(option),
            option.get("value"),
        ) else {
            continue;
        };
        if out.iter().any(|(n, _)| n == name) {
            continue;
        }
        if let Ok(value) = decode_value(name, kind, value) {
            out.push((name.to_owned(), value));
        }
    }
    let focused = focused.ok_or_else(|| OptionError::malformed("no option is focused"))?;
    Ok((out, focused))
}

/// The focused option: its name, type and raw text.
fn focus(option: &Map<String, Value>) -> Result<Focused, OptionError> {
    let name = option_name(option)?;
    let kind = option_kind(option_type(option)?)
        .ok_or_else(|| OptionError::malformed(format!("option `{name}`: not a value option")))?;
    let value = match option.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        None | Some(Value::Null) => String::new(),
        Some(_) => {
            return Err(OptionError::malformed(format!(
                "option `{name}`: the focused value is not a string, number or boolean"
            )));
        }
    };
    Ok(Focused {
        name: name.to_owned(),
        kind,
        value,
    })
}

/// The option type of a leaf type value, `None` for a subcommand, a group
/// or an unknown type.
fn option_kind(value: u64) -> Option<OptionType> {
    OptionType::ALL
        .into_iter()
        .find(|k| u64::from(k.value()) == value)
}

/// The focused value as a typed value of its option, when it parses.
fn focused_value(focused: &Focused) -> Option<OptionValue> {
    match focused.kind {
        OptionType::String => Some(OptionValue::String(focused.value.clone())),
        OptionType::Integer => focused.as_integer().map(OptionValue::Integer),
        OptionType::Number => focused.as_number().map(OptionValue::Number),
        other => decode_value(
            &focused.name,
            u64::from(other.value()),
            &Value::String(focused.value.clone()),
        )
        .ok(),
    }
}

/// What filtering a method's suggestions found, logged by the pipeline.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Warnings {
    /// `(returned, sent)` when more than the limit were returned (INT-07).
    pub(crate) truncated: Option<(usize, usize)>,
    /// How many kept choices were dropped as invalid.
    pub(crate) dropped: usize,
    /// The distinct rules the dropped choices broke, in first-seen order.
    pub(crate) rules: Vec<RuleId>,
    /// The focused option's type takes no autocomplete (OPT-11).
    pub(crate) not_choosable: bool,
    /// How many labels were cut to 100 characters with `…` (OPT-09).
    pub(crate) labels_cut: usize,
}

impl Warnings {
    /// The rule ids as `"OPT-09, OPT-03"`.
    pub(crate) fn rule_list(&self) -> String {
        self.rules
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The choices to send for an option of `kind`: the first
/// `limits.autocomplete_choices`, minus every kept choice Discord would
/// reject; none for a type without autocomplete (OPT-11).
pub(crate) fn filter_choices(
    collected: Collected,
    kind: OptionType,
    limits: &Limits,
) -> (Vec<AutocompleteChoice>, Warnings) {
    let mut warnings = Warnings::default();
    let Collected {
        mut choices,
        returned,
        labels_cut,
    } = collected;
    warnings.labels_cut = labels_cut;
    let max = limits.autocomplete_choices;
    choices.truncate(max);
    let returned = returned.max(choices.len());
    if returned > choices.len() {
        warnings.truncated = Some((returned, choices.len()));
    }
    let rules: Vec<Choice<'_>> = choices
        .iter()
        .map(|c| Choice::new(&c.name, c.value.rules()))
        .collect();
    let violations = validate_autocomplete(&rules, kind, limits);
    if violations.iter().any(|v| v.rule == RuleId::OPT_11) {
        warnings.not_choosable = true;
        return (Vec::new(), warnings);
    }
    let kept = choices.len();
    let mut bad = vec![false; kept];
    for violation in &violations {
        let Some(index) = violation
            .path
            .strip_prefix("choices[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|i| i.parse::<usize>().ok())
        else {
            continue;
        };
        if let Some(slot) = bad.get_mut(index) {
            *slot = true;
        }
        if !warnings.rules.contains(&violation.rule) {
            warnings.rules.push(violation.rule);
        }
    }
    drop(rules);
    let mut flags = bad.into_iter();
    choices.retain(|_| !flags.next().unwrap_or(false));
    warnings.dropped = kept - choices.len();
    (choices, warnings)
}

/// The type-8 callback body of `choices`.
pub(crate) fn callback_body(choices: &[AutocompleteChoice]) -> Value {
    let choices: Vec<Value> = choices
        .iter()
        .map(|c| json!({ "name": c.name, "value": c.value.json() }))
        .collect();
    json!({ "type": 8, "data": { "choices": choices } })
}

/// Decodes `json` (an autocomplete interaction) and runs the method
/// `handler` binds to its focused option over a detached context; the test
/// seam for `#[autocomplete(field)]` (see `crate::__testing`).
pub(crate) fn call_autocomplete(
    handler: &crate::CommandHandler,
    json: &[u8],
) -> BoxFuture<'static, HandlerResult<Option<Vec<AutocompleteChoice>>>> {
    let (input, focused) = match AutocompleteInput::from_json(json) {
        Ok(parsed) => parsed,
        Err(error) => return crate::command::failed(HandlerError::new(error)),
    };
    let Some(path) = handler.paths().iter().find(|p| **p == input.path()) else {
        return crate::command::unknown(input.path(), handler.name());
    };
    let Some(erased) = handler.autocomplete_for(focused.name()) else {
        return Box::pin(std::future::ready(Ok(None)));
    };
    let kind = focused.kind();
    let parts = AutocompleteParts {
        path,
        module: "",
        shard: 0,
        guild_id: None,
        channel_id: None,
        user_id: Id::new(1),
        locales: crate::locale::LocaleCell::new(crate::Locales::both(noctis_rules::Locale::EnUs)),
        focused,
        app: None,
    };
    let ctx = AutocompleteCtx::new(parts);
    let labels = ctx.labels();
    let fut = erased.call(ctx, &input);
    Box::pin(async move {
        let raw = fut.await?;
        let limits = Limits::default();
        let (choices, _) = filter_choices(collect(raw, &labels, &limits), kind, &limits);
        Ok(Some(choices))
    })
}

#[cfg(test)]
mod tests;
