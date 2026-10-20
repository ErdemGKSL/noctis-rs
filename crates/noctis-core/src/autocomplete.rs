//! Autocomplete (FR-10, AD-11; INT-06, INT-07, OPT-12): the
//! [`AutocompleteCtx`] handlers receive, the [`Focused`] option, the
//! [`AutocompleteChoice`]s they answer with, the typed
//! [`Partial<O>`](Partial) of the other options and the
//! [`AutocompleteHandler`] descriptor `#[autocomplete]` expands to.
//!
//! An autocomplete handler answers with exactly one response,
//! [`AutocompleteCtx::respond`]; there is no defer and no Responder
//! (INT-06). The pipeline sends the one type-8 callback: more than 25
//! choices are truncated with a warning (INT-07), invalid kept choices are
//! dropped with a warning, and an error, a panic or a missed 2.5 s deadline
//! answers an empty list and goes to the Autocomplete Error Sink.

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use noctis_rules::command::{Choice, ChoiceValue, OptionType, validate_autocomplete};
use noctis_rules::{Limits, RuleId};
use serde_json::{Map, Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::handler::{AppShared, HandlerResult};
use crate::options::{
    CommandInput, OptionError, OptionValue, Options, decode_interaction, decode_value, option_name,
    option_type,
};
use crate::responder::Responded;

/// How long an autocomplete handler may run, measured from when the
/// interaction arrived; then the pipeline answers an empty list. Discord
/// allows 3 s, and the rest is left for the callback.
pub(crate) const AUTOCOMPLETE_DEADLINE: Duration = Duration::from_millis(2500);

/// The option the user is typing in: its name, its type and the raw text
/// typed so far.
///
/// ```
/// # use noctis_core::{AutocompleteCtx, AutocompleteChoice, HandlerResult, Responded};
/// async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
///     let typed = ctx.focused().value().to_owned();
///     let limit = ctx.focused().as_integer().unwrap_or(0);
///     Ok(ctx.respond([AutocompleteChoice::new(format!("{typed} ({limit})"), typed)]))
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
/// value, matching the focused option's type.
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

/// One autocomplete suggestion: the name shown (1 to 100 characters) and
/// the value the option takes when it is picked.
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

/// What a handler's [`respond`](AutocompleteCtx::respond) kept: the first
/// `autocomplete_choices` choices and how many it was given.
#[derive(Debug, Default)]
pub(crate) struct Collected {
    pub(crate) choices: Vec<AutocompleteChoice>,
    pub(crate) returned: usize,
}

/// Where `respond` stores its choices for the pipeline.
pub(crate) type Slot = Arc<Mutex<Option<Collected>>>;

/// The parts of an [`AutocompleteCtx`].
pub(crate) struct AutocompleteParts {
    pub(crate) path: &'static str,
    pub(crate) module: &'static str,
    pub(crate) shard: u32,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    pub(crate) user_id: Id<UserMarker>,
    pub(crate) user_locale: Option<String>,
    pub(crate) guild_locale: Option<String>,
    pub(crate) focused: Focused,
    /// `None` in unit tests.
    pub(crate) app: Option<Arc<AppShared>>,
    pub(crate) max_choices: usize,
}

/// The context an autocomplete handler receives: the Command Path, the
/// Module, the shard, the guild, channel and user, the locales, the
/// [`Focused`] option, the App's typed state and its HTTP client.
///
/// Its only response is [`respond`](Self::respond) (INT-06): there is no
/// defer and no Responder.
///
/// ```compile_fail
/// use noctis_core::{AutocompleteCtx, HandlerResult, Responded};
///
/// async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
///     ctx.defer(); // [INT-06]
///     Ok(ctx.respond(Vec::new()))
/// }
/// ```
pub struct AutocompleteCtx {
    parts: AutocompleteParts,
    slot: Slot,
}

impl AutocompleteCtx {
    /// A context of `parts` storing into `slot`.
    pub(crate) fn new(parts: AutocompleteParts, slot: Slot) -> Self {
        Self { parts, slot }
    }

    /// The App's name (the `app` metric label), when there is an App.
    pub(crate) fn app_name(&self) -> Option<Arc<str>> {
        self.parts.app.as_ref().map(|app| Arc::clone(&app.app))
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

    /// The guild the Command is typed in, if any.
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.parts.guild_id
    }

    /// The channel the Command is typed in, if Discord sent it.
    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.parts.channel_id
    }

    /// The typing user.
    pub fn user_id(&self) -> Id<UserMarker> {
        self.parts.user_id
    }

    /// The user's client locale (`locale`), such as `"tr"`.
    pub fn user_locale(&self) -> Option<&str> {
        self.parts.user_locale.as_deref()
    }

    /// The guild's preferred locale (`guild_locale`), outside DMs.
    pub fn guild_locale(&self) -> Option<&str> {
        self.parts.guild_locale.as_deref()
    }

    /// The option being typed in.
    pub fn focused(&self) -> &Focused {
        &self.parts.focused
    }

    /// The state registered for `T` with `AppBuilder::state`, if any.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.parts.app.as_ref()?.states.get::<T>()
    }

    /// The App's one Discord HTTP client (the `Arc` `App::http` returns).
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        match &self.parts.app {
            Some(app) => &app.http,
            None => crate::command::detached_http(),
        }
    }

    /// Answers with `choices`, synchronously; the pipeline sends them as the
    /// one type-8 callback once the handler returns `Ok`.
    ///
    /// Only the first 25 (`limits.autocomplete_choices`) are kept, in order;
    /// the rest are dropped with a warning on target `noctis::autocomplete`
    /// in every limit mode (INT-07). A kept choice Discord would reject (a
    /// name outside 1..=100 characters, a STRING value over 100, a value of
    /// another type, a number out of range) is dropped with a warning.
    pub fn respond(self, choices: impl IntoIterator<Item = AutocompleteChoice>) -> Responded {
        let max = self.parts.max_choices;
        let mut kept = Vec::new();
        let mut returned = 0_usize;
        for choice in choices {
            returned += 1;
            if kept.len() < max {
                kept.push(choice);
            }
        }
        let collected = Collected {
            choices: kept,
            returned,
        };
        // A poisoned slot only follows a panic elsewhere; keep the answer.
        let mut slot = self
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(collected);
        Responded::new()
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
            .field("user_locale", &p.user_locale)
            .field("guild_locale", &p.guild_locale)
            .field("focused", &p.focused.name)
            .finish_non_exhaustive()
    }
}

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
}

/// The options of `Options` as `Option`s, what an autocomplete handler gets
/// (OPT-12); `#[derive(Options)]` generates it as `<Name>Partial`.
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
/// (OPT-12). Name it in an autocomplete handler as `partial: Partial<O>`.
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

/// A started autocomplete handler.
type AutocompleteFuture = BoxFuture<'static, HandlerResult<Responded>>;

/// The erased handler call: decodes its partial input, then runs.
type ErasedCall = dyn Fn(AutocompleteCtx, &CommandInput) -> AutocompleteFuture + Send + Sync;

/// An [`AutocompleteHandler`] with its options type erased; what a
/// `CommandHandler` looks up per focused option.
#[derive(Clone)]
pub(crate) struct ErasedAutocomplete {
    name: &'static str,
    call: Arc<ErasedCall>,
}

impl ErasedAutocomplete {
    /// The handler's path (`module_path!()::name`).
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    /// Starts the handler for `ctx` over `input`.
    pub(crate) fn call(&self, ctx: AutocompleteCtx, input: &CommandInput) -> AutocompleteFuture {
        (self.call)(ctx, input)
    }
}

/// The erased handler of the option `option` of `O`, if one is linked;
/// what a `CommandHandler` with options stores.
pub(crate) fn erased_for<O: Options>(option: &str) -> Option<ErasedAutocomplete> {
    O::autocomplete(option).map(|h| h.erased)
}

/// One autocomplete handler for an option of the options struct `O`, as
/// `#[autocomplete]` builds it; link it with
/// `#[option(autocomplete = path)]` (no Module registration).
///
/// Annotating `async fn suggest` with `#[autocomplete]` gives a
/// `fn suggest()` that returns its `AutocompleteHandler`. Cheap to clone;
/// `Debug` prints the name.
pub struct AutocompleteHandler<O> {
    erased: ErasedAutocomplete,
    _options: PhantomData<fn() -> O>,
}

impl<O> Clone for AutocompleteHandler<O> {
    fn clone(&self) -> Self {
        Self {
            erased: self.erased.clone(),
            _options: PhantomData,
        }
    }
}

impl<O> fmt::Debug for AutocompleteHandler<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AutocompleteHandler")
            .field(&self.erased.name)
            .finish()
    }
}

impl<O> AutocompleteHandler<O> {
    /// The handler's path (`module_path!()::name`).
    pub fn name(&self) -> &'static str {
        self.erased.name
    }

    fn from_call(name: &'static str, call: Arc<ErasedCall>) -> Self {
        Self {
            erased: ErasedAutocomplete { name, call },
            _options: PhantomData,
        }
    }
}

impl<O: Options> AutocompleteHandler<O> {
    /// A handler taking the partial options `P` of `O`; what
    /// `#[autocomplete]` expands to for `(ctx, partial: Partial<O>)`.
    #[doc(hidden)]
    pub fn __new<P, F, Fut>(name: &'static str, f: F) -> Self
    where
        P: PartialOptions<Options = O>,
        F: Fn(AutocompleteCtx, P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        let call = move |ctx: AutocompleteCtx, input: &CommandInput| -> AutocompleteFuture {
            let partial = P::__from_input(input);
            Box::pin(f(ctx, partial))
        };
        Self::from_call(name, Arc::new(call))
    }

    /// A handler taking only the context, usable for any options struct
    /// `O`; what `#[autocomplete]` expands to for `(ctx)`.
    #[doc(hidden)]
    pub fn __new_plain<F, Fut>(name: &'static str, f: F) -> Self
    where
        F: Fn(AutocompleteCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        let call = move |ctx: AutocompleteCtx, _: &CommandInput| -> AutocompleteFuture {
            Box::pin(f(ctx))
        };
        Self::from_call(name, Arc::new(call))
    }
}

/// A descriptor `fn() -> AutocompleteHandler<O>`: what
/// `#[option(autocomplete = path)]` accepts.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an `#[autocomplete]` handler for `{O}`",
    label = "not an `#[autocomplete]` handler for `{O}`",
    note = "annotate an `async fn(ctx: AutocompleteCtx[, partial: Partial<{O}>]) -> HandlerResult<Responded>` with `#[autocomplete]`"
)]
pub trait __AutocompleteFor<O> {
    /// Builds the handler.
    fn __handler(self) -> AutocompleteHandler<O>;
}

/// What an `#[autocomplete]` descriptor returns for `O`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`autocomplete = …` is not an `#[autocomplete]` handler for `{O}`",
    label = "its descriptor returns `{Self}`, not `AutocompleteHandler<{O}>`",
    note = "annotate an `async fn(ctx: AutocompleteCtx[, partial: Partial<{O}>]) -> HandlerResult<Responded>` with `#[autocomplete]`"
)]
pub trait __AutocompleteOutput<O> {
    /// The handler.
    fn __into_handler(self) -> AutocompleteHandler<O>;
}

impl<O> __AutocompleteOutput<O> for AutocompleteHandler<O> {
    fn __into_handler(self) -> AutocompleteHandler<O> {
        self
    }
}

impl<O, F, R> __AutocompleteFor<O> for F
where
    F: FnOnce() -> R,
    R: __AutocompleteOutput<O>,
{
    fn __handler(self) -> AutocompleteHandler<O> {
        self().__into_handler()
    }
}

/// The handler of the descriptor `f` for `O`; what `#[derive(Options)]`
/// expands `autocomplete = path` to.
#[doc(hidden)]
pub fn __autocomplete_for<O: Options, F: __AutocompleteFor<O>>(f: F) -> AutocompleteHandler<O> {
    f.__handler()
}

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
                "option `{name}`: the focused value is not a string or number"
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

/// What filtering a handler's choices found, logged by the pipeline.
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
    } = collected;
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

/// Decodes `json` (an autocomplete interaction) and runs the handler
/// `handler` links to its focused option over a detached context; the test
/// seam for `#[autocomplete]` (see `crate::__testing`).
pub(crate) fn call_autocomplete(
    handler: &crate::CommandHandler,
    json: &[u8],
) -> BoxFuture<'static, HandlerResult<Option<Vec<AutocompleteChoice>>>> {
    let (input, focused) = match AutocompleteInput::from_json(json) {
        Ok(parsed) => parsed,
        Err(error) => return crate::command::failed(crate::HandlerError::new(error)),
    };
    let Some(path) = handler.paths().iter().find(|p| **p == input.path()) else {
        return crate::command::unknown(input.path(), handler.name());
    };
    let Some(erased) = handler.autocomplete_for(focused.name()) else {
        return Box::pin(std::future::ready(Ok(None)));
    };
    let slot = Slot::default();
    let parts = AutocompleteParts {
        path,
        module: "",
        shard: 0,
        guild_id: None,
        channel_id: None,
        user_id: Id::new(1),
        user_locale: None,
        guild_locale: None,
        focused,
        app: None,
        max_choices: Limits::default().autocomplete_choices,
    };
    let fut = erased.call(AutocompleteCtx::new(parts, Arc::clone(&slot)), &input);
    Box::pin(async move {
        let _responded = fut.await?;
        let collected = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(Some(collected.map(|c| c.choices).unwrap_or_default()))
    })
}

#[cfg(test)]
mod tests;
