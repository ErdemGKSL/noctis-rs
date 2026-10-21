//! Typed chat-input command options: the [`CommandOption`] trait of option
//! field types, the [`Options`] trait `#[derive(SlashCommand)]` implements, the
//! decoded [`CommandInput`] of an interaction and the typed errors
//! [`OptionError`] and [`InvalidOption`] (FR-10, FR-77, AD-4, AD-26).
//!
//! Decoding never panics: every malformed, missing or mistyped value is an
//! [`OptionError`], and the handler does not run.

use std::collections::HashMap;

use noctis_rules::MessageKey;
use noctis_rules::command::{Choice, OptionDecl, OptionType};
use noctis_ui::{Mentionable, ResolvedAttachment, ResolvedChannel, ResolvedRole, ResolvedUser};
use serde_json::{Map, Number, Value};
use twilight_model::application::interaction::InteractionDataResolved;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    AttachmentMarker, ChannelMarker, GenericMarker, RoleMarker, UserMarker,
};

use crate::autocomplete::{__AutocompleteMethod, PartialOptions};
use crate::command::CommandCtx;

/// A field type of a `#[derive(SlashCommand)]` struct: its Discord option type
/// (OPT-02), its predefined choices and how it decodes from an
/// [`OptionValue`].
///
/// Implemented for `String` (STRING), `i64` (INTEGER), `f64` (NUMBER),
/// `bool` (BOOLEAN), `Id<UserMarker>` and [`ResolvedUser`] (USER),
/// `Id<ChannelMarker>` and [`ResolvedChannel`] (CHANNEL), `Id<RoleMarker>`
/// and [`ResolvedRole`] (ROLE), `Id<GenericMarker>` and [`Mentionable`]
/// (MENTIONABLE), `Id<AttachmentMarker>` and [`ResolvedAttachment`]
/// (ATTACHMENT), and by `#[derive(Choices)]` (STRING with choices). Any
/// other type is a compile error naming OPT-02.
///
/// ```
/// use noctis_core::CommandOption;
/// use noctis_rules::command::OptionType;
///
/// assert_eq!(<i64 as CommandOption>::KIND, OptionType::Integer);
/// assert!(<String as CommandOption>::CHOICES.is_empty());
/// ```
#[diagnostic::on_unimplemented(
    message = "[OPT-02] `{Self}` is not a command option type",
    label = "not a command option type",
    note = "option fields are String, i64, f64, bool, Id<UserMarker>, Id<ChannelMarker>, Id<RoleMarker>, Id<GenericMarker>, Id<AttachmentMarker>, ResolvedUser, ResolvedChannel, ResolvedRole, Mentionable, ResolvedAttachment or a #[derive(Choices)] enum; wrap optional ones in Option<T>"
)]
pub trait CommandOption: Sized + Send + Sync + 'static {
    /// The Discord option type.
    const KIND: OptionType;

    /// The predefined choices (empty for free input).
    const CHOICES: &'static [Choice<'static>] = &[];

    /// Decodes a value of kind [`KIND`](Self::KIND) of the option `option`;
    /// `input` supplies the resolved entities.
    ///
    /// # Errors
    ///
    /// [`OptionError::WrongType`] for a value of another kind,
    /// [`OptionError::Unresolved`] for an entity missing from the resolved
    /// data, [`OptionError::UnknownChoice`] for a value outside the choices.
    fn decode(value: &OptionValue, input: &CommandInput, option: &str)
    -> Result<Self, OptionError>;
}

/// The typed options of a slash Command: the named fields of a
/// `#[derive(SlashCommand)]` struct, which implements it.
///
/// The struct is decoded from the interaction, then every validator runs,
/// and only then does its `#[handler]` method run on it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a slash Command's options",
    label = "not a `#[derive(SlashCommand)]` struct",
    note = "derive it with `#[derive(SlashCommand)]`"
)]
pub trait Options: Sized + Send + 'static {
    /// The option declarations, in field order.
    const OPTIONS: &'static [OptionDecl<'static>];

    /// The same fields, each an `Option`: what autocomplete methods get as
    /// [`Partial<Self>`](crate::Partial) (OPT-12).
    /// `#[derive(SlashCommand)]` generates it as `<Struct>Partial`.
    type Partial: PartialOptions<Options = Self>;

    /// Decodes the struct from an interaction's leaf options.
    ///
    /// # Errors
    ///
    /// The first field's [`OptionError`], in declaration order.
    fn from_input(input: &CommandInput) -> Result<Self, OptionError>;

    /// Runs the `validate = path` validators of the present fields, in
    /// declaration order.
    ///
    /// # Errors
    ///
    /// The first failing validator's [`InvalidOption`].
    fn validate(&self, ctx: &CommandCtx) -> Result<(), InvalidOption> {
        let _ = ctx;
        Ok(())
    }

    /// The `#[autocomplete(field)]` method bound to the option `option`
    /// (flagged `#[option(.., autocomplete)]`), if any; the method table
    /// `#[derive(SlashCommand)]` generates.
    #[doc(hidden)]
    fn __autocomplete(option: &str) -> Option<__AutocompleteMethod> {
        let _ = option;
        None
    }
}

/// One decoded option value, by Discord option type.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    /// STRING (3).
    String(String),
    /// INTEGER (4).
    Integer(i64),
    /// NUMBER (10).
    Number(f64),
    /// BOOLEAN (5).
    Boolean(bool),
    /// USER (6).
    User(Id<UserMarker>),
    /// CHANNEL (7).
    Channel(Id<ChannelMarker>),
    /// ROLE (8).
    Role(Id<RoleMarker>),
    /// MENTIONABLE (9).
    Mentionable(Id<GenericMarker>),
    /// ATTACHMENT (11).
    Attachment(Id<AttachmentMarker>),
}

impl OptionValue {
    /// The value's option type.
    ///
    /// ```
    /// use noctis_core::OptionValue;
    /// use noctis_rules::command::OptionType;
    ///
    /// assert_eq!(OptionValue::Integer(3).kind(), OptionType::Integer);
    /// ```
    pub fn kind(&self) -> OptionType {
        match self {
            Self::String(_) => OptionType::String,
            Self::Integer(_) => OptionType::Integer,
            Self::Number(_) => OptionType::Number,
            Self::Boolean(_) => OptionType::Boolean,
            Self::User(_) => OptionType::User,
            Self::Channel(_) => OptionType::Channel,
            Self::Role(_) => OptionType::Role,
            Self::Mentionable(_) => OptionType::Mentionable,
            Self::Attachment(_) => OptionType::Attachment,
        }
    }
}

/// Why options could not be decoded; the handler does not run.
///
/// ```
/// use noctis_core::OptionError;
///
/// let err = OptionError::Missing { option: "member".into() };
/// assert_eq!(err.to_string(), "required option `member` is missing");
/// ```
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum OptionError {
    /// The interaction JSON is not a well-formed command interaction.
    #[error("malformed command interaction: {reason}")]
    Malformed {
        /// What is wrong.
        reason: String,
    },
    /// The interaction is not a chat-input (slash) command.
    #[error(
        "not a slash command: interaction type {interaction_type}, command type {command_type}"
    )]
    NotSlashCommand {
        /// The interaction `type` (2 for an application command).
        interaction_type: u64,
        /// `data.type` (1 for a chat-input command; 1 when absent).
        command_type: u64,
    },
    /// An option name appears twice among the leaf options.
    #[error("option `{option}` is sent twice")]
    Duplicate {
        /// The option name.
        option: String,
    },
    /// A required option is absent.
    #[error("required option `{option}` is missing")]
    Missing {
        /// The option name.
        option: String,
    },
    /// The option arrived with another type than its field's (OPT-02).
    #[error("[OPT-02] option `{option}`: expected {}, found {}", expected.name(), found.name())]
    WrongType {
        /// The option name.
        option: String,
        /// The field's option type.
        expected: OptionType,
        /// The type that was sent.
        found: OptionType,
    },
    /// An INTEGER value that does not fit `i64` (OPT-03).
    #[error("[OPT-03] option `{option}`: {value} is not an i64")]
    OutOfRange {
        /// The option name.
        option: String,
        /// The JSON number as text.
        value: String,
    },
    /// A user, role, channel, mentionable or attachment id missing from the
    /// resolved data.
    #[error("option `{option}`: id {id} is not in the resolved data")]
    Unresolved {
        /// The option name.
        option: String,
        /// The id.
        id: u64,
    },
    /// A STRING value that is none of the Choices enum's values.
    #[error("option `{option}`: `{value}` is not one of its choices")]
    UnknownChoice {
        /// The option name.
        option: String,
        /// The value that was sent.
        value: String,
    },
}

impl OptionError {
    pub(crate) fn malformed(reason: impl Into<String>) -> Self {
        Self::Malformed {
            reason: reason.into(),
        }
    }
}

/// A `validate = path` validator rejected an option; carries the validator's
/// [`MessageKey`] (the ephemeral notice is sent by the Command pipeline).
///
/// ```
/// use noctis_core::InvalidOption;
/// use noctis_rules::MessageKey;
///
/// let err = InvalidOption::__new("days", MessageKey::new("warn.days.positive"));
/// assert_eq!(err.option(), "days");
/// assert_eq!(err.to_string(), "option `days` failed validation: warn.days.positive");
/// ```
#[derive(Debug, Clone, thiserror::Error)]
#[error("option `{option}` failed validation: {key}")]
pub struct InvalidOption {
    option: &'static str,
    key: MessageKey,
}

impl InvalidOption {
    /// The failure of the option `option`; what `#[derive(SlashCommand)]` builds.
    #[doc(hidden)]
    pub fn __new(option: &'static str, key: MessageKey) -> Self {
        Self { option, key }
    }

    /// The option's name.
    pub fn option(&self) -> &'static str {
        self.option
    }

    /// The validator's message key.
    pub fn key(&self) -> MessageKey {
        self.key
    }
}

/// The decoded data of a chat-input command interaction: the invoked
/// Command Path, the leaf options (those of the invoked subcommand) and the
/// resolved entities.
///
/// ```
/// use noctis_core::{CommandInput, OptionValue};
///
/// let json = br#"{"type":2,"data":{"name":"warn","type":1,"options":[{"name":"days","type":4,"value":3}]}}"#;
/// let input = CommandInput::from_json(json).unwrap();
/// assert_eq!(input.path(), "warn");
/// assert_eq!(input.get("days"), Some(&OptionValue::Integer(3)));
/// ```
#[derive(Clone, Debug)]
pub struct CommandInput {
    path: String,
    options: Vec<(String, OptionValue)>,
    resolved: InteractionDataResolved,
}

impl CommandInput {
    /// Decodes an interaction object (the gateway `d` or the HTTP body).
    ///
    /// Unknown fields and `focused` are ignored. Snowflakes must be strings,
    /// as Discord sends them.
    ///
    /// # Errors
    ///
    /// [`OptionError::NotSlashCommand`] for another interaction or command
    /// type, [`OptionError::Duplicate`] for a repeated leaf option,
    /// [`OptionError::OutOfRange`] for an INTEGER outside `i64` and
    /// [`OptionError::Malformed`] for anything else that is not well formed.
    pub fn from_json(raw: &[u8]) -> Result<Self, OptionError> {
        let (path, options, resolved) = decode_interaction(raw, 2, leaf_options)?;
        Ok(Self {
            path,
            options,
            resolved,
        })
    }

    /// An input of already decoded parts (the autocomplete decoder).
    pub(crate) fn from_parts(
        path: String,
        options: Vec<(String, OptionValue)>,
        resolved: InteractionDataResolved,
    ) -> Self {
        Self {
            path,
            options,
            resolved,
        }
    }

    /// An input without options or resolved data, for a direct call of
    /// `path`.
    pub(crate) fn empty(path: &str) -> Self {
        Self::new(path)
    }

    /// An input for the Command Path `path` (words joined by one ASCII
    /// space), without options or resolved data; what a
    /// [`Dispatcher`](crate::Dispatcher) caller builds after decoding its own
    /// options.
    ///
    /// ```
    /// use noctis_core::CommandInput;
    ///
    /// let input = CommandInput::new("config set prefix");
    /// assert_eq!(input.path(), "config set prefix");
    /// assert_eq!(input.get("value"), None);
    /// ```
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            options: Vec::new(),
            resolved: empty_resolved(),
        }
    }

    /// Adds the leaf option `name` with `value`, replacing an earlier value
    /// of the same name.
    ///
    /// ```
    /// use noctis_core::{CommandInput, OptionValue};
    ///
    /// let input = CommandInput::new("warn")
    ///     .with_option("days", OptionValue::Integer(1))
    ///     .with_option("days", OptionValue::Integer(3));
    /// assert_eq!(input.get("days"), Some(&OptionValue::Integer(3)));
    /// ```
    #[must_use]
    pub fn with_option(mut self, name: impl Into<String>, value: OptionValue) -> Self {
        let name = name.into();
        match self.options.iter_mut().find(|(n, _)| *n == name) {
            Some((_, slot)) => *slot = value,
            None => self.options.push((name, value)),
        }
        self
    }

    /// The invoked Command Path, words joined by one ASCII space.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The value of the leaf option `name`, if it was sent.
    pub fn get(&self, name: &str) -> Option<&OptionValue> {
        self.options.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// The resolved users, members, roles, channels and attachments.
    pub fn resolved(&self) -> &InteractionDataResolved {
        &self.resolved
    }
}

/// Decodes a chat-input interaction object of interaction type
/// `interaction_kind` (2 for a command, 4 for autocomplete): its Command
/// Path, its leaf options through `leaf` and its resolved data.
///
/// # Errors
///
/// [`OptionError::NotSlashCommand`] for another interaction or command
/// type, `leaf`'s error, and [`OptionError::Malformed`] for anything else
/// that is not well formed.
pub(crate) fn decode_interaction<T>(
    raw: &[u8],
    interaction_kind: u64,
    leaf: impl FnOnce(Vec<&Map<String, Value>>) -> Result<T, OptionError>,
) -> Result<(String, T, InteractionDataResolved), OptionError> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|e| OptionError::malformed(format!("invalid JSON: {e}")))?;
    let Value::Object(mut root) = value else {
        return Err(OptionError::malformed("the interaction is not an object"));
    };
    let interaction_type = root
        .get("type")
        .and_then(Value::as_u64)
        .ok_or_else(|| OptionError::malformed("missing interaction `type`"))?;
    let data = match root.remove("data") {
        Some(Value::Object(data)) => Some(data),
        Some(_) => return Err(OptionError::malformed("`data` is not an object")),
        None => None,
    };
    let command_type = match data.as_ref().and_then(|d| d.get("type")) {
        None | Some(Value::Null) => 1,
        Some(t) => t
            .as_u64()
            .ok_or_else(|| OptionError::malformed("`data.type` is not an integer"))?,
    };
    if interaction_type != interaction_kind || command_type != 1 {
        return Err(OptionError::NotSlashCommand {
            interaction_type,
            command_type,
        });
    }
    let mut data = data.ok_or_else(|| OptionError::malformed("missing `data`"))?;
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| OptionError::malformed("missing `data.name`"))?;
    let mut path = name.to_owned();
    let options = options_array(data.get("options"), "data.options")?;
    let options = walk_path(options, &mut path)?;
    let options = leaf(options)?;
    let resolved = match data.remove("resolved") {
        None | Some(Value::Null) => empty_resolved(),
        Some(r) => serde_json::from_value(r)
            .map_err(|e| OptionError::malformed(format!("bad `resolved` data: {e}")))?,
    };
    Ok((path, options, resolved))
}

pub(crate) fn empty_resolved() -> InteractionDataResolved {
    InteractionDataResolved {
        attachments: HashMap::new(),
        channels: HashMap::new(),
        members: HashMap::new(),
        messages: HashMap::new(),
        roles: HashMap::new(),
        users: HashMap::new(),
    }
}

/// The objects of an `options` array (empty when absent or null).
fn options_array<'a>(
    value: Option<&'a Value>,
    what: &str,
) -> Result<Vec<&'a Map<String, Value>>, OptionError> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_object().ok_or_else(|| {
                    OptionError::malformed(format!("an entry of `{what}` is not an object"))
                })
            })
            .collect(),
        Some(_) => Err(OptionError::malformed(format!("`{what}` is not an array"))),
    }
}

pub(crate) fn option_name(option: &Map<String, Value>) -> Result<&str, OptionError> {
    option
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| OptionError::malformed("an option has no string `name`"))
}

pub(crate) fn option_type(option: &Map<String, Value>) -> Result<u64, OptionError> {
    option
        .get("type")
        .and_then(Value::as_u64)
        .ok_or_else(|| OptionError::malformed("an option has no integer `type`"))
}

/// Follows a subcommand (type 1) or group (type 2) down to the leaf
/// options, appending the words to `path`.
fn walk_path<'a>(
    options: Vec<&'a Map<String, Value>>,
    path: &mut String,
) -> Result<Vec<&'a Map<String, Value>>, OptionError> {
    let Some(&first) = options.first() else {
        return Ok(options);
    };
    let first_type = option_type(first)?;
    if !matches!(first_type, 1 | 2) {
        return Ok(options);
    }
    if options.len() != 1 {
        return Err(OptionError::malformed(
            "a subcommand or group must be the only option",
        ));
    }
    path.push(' ');
    path.push_str(option_name(first)?);
    let children = options_array(first.get("options"), "options")?;
    if first_type == 1 {
        return Ok(children);
    }
    let [child] = children.as_slice() else {
        return Err(OptionError::malformed(
            "a subcommand group must hold exactly one subcommand",
        ));
    };
    if option_type(child)? != 1 {
        return Err(OptionError::malformed(
            "a subcommand group must hold a subcommand",
        ));
    }
    path.push(' ');
    path.push_str(option_name(child)?);
    options_array(child.get("options"), "options")
}

/// Decodes the leaf options, rejecting duplicates.
fn leaf_options(
    options: Vec<&Map<String, Value>>,
) -> Result<Vec<(String, OptionValue)>, OptionError> {
    let mut out: Vec<(String, OptionValue)> = Vec::with_capacity(options.len());
    for option in options {
        let name = option_name(option)?;
        let kind = option_type(option)?;
        if out.iter().any(|(n, _)| n == name) {
            return Err(OptionError::Duplicate {
                option: name.to_owned(),
            });
        }
        let value = option
            .get("value")
            .ok_or_else(|| OptionError::malformed(format!("option `{name}` has no `value`")))?;
        let value = decode_value(name, kind, value)?;
        out.push((name.to_owned(), value));
    }
    Ok(out)
}

pub(crate) fn decode_value(
    name: &str,
    kind: u64,
    value: &Value,
) -> Result<OptionValue, OptionError> {
    let wrong = |what: &str| OptionError::malformed(format!("option `{name}`: {what}"));
    Ok(match kind {
        1 | 2 => return Err(wrong("a subcommand or group next to value options")),
        3 => OptionValue::String(
            value
                .as_str()
                .ok_or_else(|| wrong("a STRING value is not a string"))?
                .to_owned(),
        ),
        4 => {
            let Value::Number(n) = value else {
                return Err(wrong("an INTEGER value is not a number"));
            };
            OptionValue::Integer(integer(name, n)?)
        }
        5 => OptionValue::Boolean(
            value
                .as_bool()
                .ok_or_else(|| wrong("a BOOLEAN value is not a boolean"))?,
        ),
        6 => OptionValue::User(snowflake(value).ok_or_else(|| wrong("not a snowflake string"))?),
        7 => OptionValue::Channel(snowflake(value).ok_or_else(|| wrong("not a snowflake string"))?),
        8 => OptionValue::Role(snowflake(value).ok_or_else(|| wrong("not a snowflake string"))?),
        9 => OptionValue::Mentionable(
            snowflake(value).ok_or_else(|| wrong("not a snowflake string"))?,
        ),
        10 => OptionValue::Number(
            value
                .as_f64()
                .ok_or_else(|| wrong("a NUMBER value is not a number"))?,
        ),
        11 => OptionValue::Attachment(
            snowflake(value).ok_or_else(|| wrong("not a snowflake string"))?,
        ),
        other => return Err(wrong(&format!("unknown option type {other}"))),
    })
}

/// An INTEGER value as `i64`, or OPT-03 [`OptionError::OutOfRange`].
fn integer(name: &str, n: &Number) -> Result<i64, OptionError> {
    n.as_i64().ok_or_else(|| OptionError::OutOfRange {
        option: name.to_owned(),
        value: n.to_string(),
    })
}

/// A non-zero snowflake sent as a decimal string.
fn snowflake<M>(value: &Value) -> Option<Id<M>> {
    let text = value.as_str()?;
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Id::new_checked(text.parse().ok()?)
}

fn wrong_type<T: CommandOption>(value: &OptionValue, option: &str) -> OptionError {
    OptionError::WrongType {
        option: option.to_owned(),
        expected: T::KIND,
        found: value.kind(),
    }
}

fn unresolved(option: &str, id: u64) -> OptionError {
    OptionError::Unresolved {
        option: option.to_owned(),
        id,
    }
}

/// Implements [`CommandOption`] for a type decoded from one value variant.
macro_rules! plain_option {
    ($($ty:ty => $kind:ident, $variant:ident($v:ident) => $make:expr;)*) => {$(
        impl CommandOption for $ty {
            const KIND: OptionType = OptionType::$kind;

            fn decode(
                value: &OptionValue,
                input: &CommandInput,
                option: &str,
            ) -> Result<Self, OptionError> {
                let _ = input;
                match value {
                    OptionValue::$variant($v) => $make,
                    other => Err(wrong_type::<Self>(other, option)),
                }
            }
        }
    )*};
}

plain_option! {
    String => String, String(v) => Ok(v.clone());
    i64 => Integer, Integer(v) => Ok(*v);
    f64 => Number, Number(v) => Ok(*v);
    bool => Boolean, Boolean(v) => Ok(*v);
    Id<UserMarker> => User, User(v) => Ok(*v);
    Id<ChannelMarker> => Channel, Channel(v) => Ok(*v);
    Id<RoleMarker> => Role, Role(v) => Ok(*v);
    Id<GenericMarker> => Mentionable, Mentionable(v) => Ok(*v);
    Id<AttachmentMarker> => Attachment, Attachment(v) => Ok(*v);
}

/// Implements [`CommandOption`] for a resolved entity type.
macro_rules! resolved_option {
    ($($ty:ty => $kind:ident, $variant:ident;)*) => {$(
        impl CommandOption for $ty {
            const KIND: OptionType = OptionType::$kind;

            fn decode(
                value: &OptionValue,
                input: &CommandInput,
                option: &str,
            ) -> Result<Self, OptionError> {
                match value {
                    OptionValue::$variant(id) => <$ty>::from_resolved(input.resolved(), *id)
                        .ok_or_else(|| unresolved(option, id.get())),
                    other => Err(wrong_type::<Self>(other, option)),
                }
            }
        }
    )*};
}

resolved_option! {
    ResolvedUser => User, User;
    ResolvedChannel => Channel, Channel;
    ResolvedRole => Role, Role;
    Mentionable => Mentionable, Mentionable;
    ResolvedAttachment => Attachment, Attachment;
}

/// Helpers `#[derive(SlashCommand)]` and `#[derive(Choices)]` expand to; not
/// public API.
#[doc(hidden)]
pub mod __options {
    use super::{CommandInput, CommandOption, OptionError, OptionValue, wrong_type};

    /// The required option `name`: [`OptionError::Missing`] when absent.
    ///
    /// # Errors
    ///
    /// `Missing`, `WrongType`, or the type's decode error.
    pub fn required<T: CommandOption>(
        input: &CommandInput,
        name: &'static str,
    ) -> Result<T, OptionError> {
        optional(input, name)?.ok_or_else(|| OptionError::Missing {
            option: name.to_owned(),
        })
    }

    /// The optional option `name`: `None` when absent.
    ///
    /// # Errors
    ///
    /// `WrongType`, or the type's decode error.
    pub fn optional<T: CommandOption>(
        input: &CommandInput,
        name: &'static str,
    ) -> Result<Option<T>, OptionError> {
        let Some(value) = input.get(name) else {
            return Ok(None);
        };
        if value.kind() != T::KIND {
            return Err(wrong_type::<T>(value, name));
        }
        T::decode(value, input, name).map(Some)
    }

    /// The option `name` of an autocomplete partial input: `None` when it
    /// is absent, mistyped, unresolved or not one of its choices (OPT-12).
    pub fn partial<T: CommandOption>(input: &CommandInput, name: &'static str) -> Option<T> {
        optional(input, name).ok().flatten()
    }

    /// Decodes a Choices enum from a STRING value.
    ///
    /// # Errors
    ///
    /// `WrongType` for a non-STRING value, `UnknownChoice` for an unknown
    /// one.
    pub fn choice<C: CommandOption + noctis_ui::Choices>(
        value: &OptionValue,
        option: &str,
    ) -> Result<C, OptionError> {
        let OptionValue::String(text) = value else {
            return Err(wrong_type::<C>(value, option));
        };
        C::from_value(text).ok_or_else(|| OptionError::UnknownChoice {
            option: option.to_owned(),
            value: text.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::__options::{choice, optional, required};
    use super::*;

    fn input(value: &Value) -> Result<CommandInput, OptionError> {
        CommandInput::from_json(value.to_string().as_bytes())
    }

    fn slash(data: &Value) -> Value {
        json!({ "type": 2, "data": data })
    }

    fn leaf(options: &Value) -> Result<CommandInput, OptionError> {
        input(&slash(
            &json!({ "name": "warn", "type": 1, "options": options }),
        ))
    }

    fn ok(result: Result<CommandInput, OptionError>) -> CommandInput {
        result.unwrap_or_else(|e| panic!("{e}"))
    }

    fn malformed(result: Result<CommandInput, OptionError>) -> String {
        match result {
            Err(OptionError::Malformed { reason }) => reason,
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn top_level_command() {
        let i = ok(leaf(&json!([{ "name": "days", "type": 4, "value": 3 }])));
        assert_eq!(i.path(), "warn");
        assert_eq!(i.get("days"), Some(&OptionValue::Integer(3)));
        assert_eq!(i.get("nope"), None);
    }

    #[test]
    fn subcommand_and_group_paths() {
        let i = ok(input(&slash(&json!({
            "name": "mod", "type": 1,
            "options": [{ "type": 1, "name": "warn", "options": [
                { "name": "reason", "type": 3, "value": "spam" }
            ]}]
        }))));
        assert_eq!(i.path(), "mod warn");
        assert_eq!(i.get("reason"), Some(&OptionValue::String("spam".into())));
        let i = ok(input(&slash(&json!({
            "name": "settings",
            "options": [{ "type": 2, "name": "logs", "options": [
                { "type": 1, "name": "channel", "options": [] }
            ]}]
        }))));
        assert_eq!(i.path(), "settings logs channel");
        assert!(i.options.is_empty());
    }

    #[test]
    fn no_options_and_absent_type() {
        let i = ok(input(&slash(&json!({ "name": "ping" }))));
        assert_eq!(i.path(), "ping");
        assert!(i.options.is_empty());
        assert!(i.resolved().users.is_empty());
    }

    #[test]
    fn not_a_slash_command() {
        for (value, it, ct) in [
            (json!({ "type": 3, "data": { "custom_id": "x" } }), 3, 1),
            (json!({ "type": 4, "data": { "name": "a" } }), 4, 1),
            (
                json!({ "type": 2, "data": { "name": "a", "type": 2 } }),
                2,
                2,
            ),
        ] {
            let err = input(&value).unwrap_err();
            assert_eq!(
                err,
                OptionError::NotSlashCommand {
                    interaction_type: it,
                    command_type: ct
                }
            );
        }
        assert_eq!(
            input(&json!({ "type": 3 })).unwrap_err().to_string(),
            "not a slash command: interaction type 3, command type 1"
        );
    }

    #[test]
    fn malformed_inputs() {
        assert!(malformed(CommandInput::from_json(b"{")).starts_with("invalid JSON"));
        assert!(
            OptionError::malformed("x")
                .to_string()
                .starts_with("malformed command interaction: ")
        );
        malformed(input(&json!({ "type": 2 })));
        malformed(input(&slash(&json!({ "type": 1 }))));
        malformed(input(&slash(&json!({ "name": "a", "options": {} }))));
        malformed(leaf(&json!([{ "type": 4, "value": 1 }])));
        malformed(leaf(&json!([{ "name": "a", "value": 1 }])));
        malformed(leaf(&json!([{ "name": "a", "type": 12, "value": 1 }])));
        malformed(leaf(&json!([
            { "name": "a", "type": 4, "value": 1 },
            { "name": "s", "type": 1, "options": [] }
        ])));
        malformed(leaf(&json!([
            { "name": "s", "type": 1, "options": [] },
            { "name": "a", "type": 4, "value": 1 }
        ])));
        malformed(leaf(&json!([{ "name": "g", "type": 2, "options": [
            { "name": "a", "type": 1 }, { "name": "b", "type": 1 }
        ]}])));
        malformed(leaf(&json!([{ "name": "g", "type": 2, "options": [
            { "name": "a", "type": 4, "value": 1 }
        ]}])));
        for (kind, value) in [
            (6, json!("abc")),
            (6, json!("0")),
            (6, json!(80_351_110_224_678_912_u64)),
            (3, json!(5)),
            (5, json!("true")),
            (4, json!("3")),
            (10, json!("2.5")),
        ] {
            malformed(leaf(
                &json!([{ "name": "a", "type": kind, "value": value }]),
            ));
        }
        malformed(leaf(&json!([{ "name": "a", "type": 4 }])));
        malformed(input(&slash(
            &json!({ "name": "a", "resolved": { "users": 3 } }),
        )));
    }

    #[test]
    fn duplicate_option() {
        let err = leaf(&json!([
            { "name": "days", "type": 4, "value": 1 },
            { "name": "days", "type": 4, "value": 2 }
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            OptionError::Duplicate {
                option: "days".into()
            }
        );
    }

    #[test]
    fn opt_03_integer_outside_i64() {
        let err =
            leaf(&json!([{ "name": "days", "type": 4, "value": 9_223_372_036_854_775_808_u64 }]))
                .unwrap_err();
        assert_eq!(
            err.to_string(),
            "[OPT-03] option `days`: 9223372036854775808 is not an i64"
        );
        for raw in ["-9223372036854775809", "1.5", "1e3"] {
            let json = format!(
                r#"{{"type":2,"data":{{"name":"w","options":[{{"name":"days","type":4,"value":{raw}}}]}}}}"#
            );
            let err = CommandInput::from_json(json.as_bytes()).unwrap_err();
            assert!(
                matches!(err, OptionError::OutOfRange { .. }),
                "{raw}: {err:?}"
            );
        }
        for (raw, expected) in [
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
            ("9007199254740993", 9_007_199_254_740_993),
        ] {
            let json = format!(
                r#"{{"type":2,"data":{{"name":"w","options":[{{"name":"days","type":4,"value":{raw}}}]}}}}"#
            );
            let i = ok(CommandInput::from_json(json.as_bytes()));
            assert_eq!(i.get("days"), Some(&OptionValue::Integer(expected)));
        }
    }

    #[test]
    fn opt_04_numbers() {
        for (raw, expected) in [("2", 2.0), ("2.5", 2.5), ("-0.0", -0.0), ("1e308", 1e308)] {
            let json = format!(
                r#"{{"type":2,"data":{{"name":"w","options":[{{"name":"n","type":10,"value":{raw}}}]}}}}"#
            );
            let i = ok(CommandInput::from_json(json.as_bytes()));
            let Some(OptionValue::Number(n)) = i.get("n") else {
                panic!("{raw}");
            };
            assert_eq!(n.to_bits(), f64::to_bits(expected), "{raw}");
        }
    }

    #[test]
    fn snowflakes_focused_and_unicode() {
        let i = ok(leaf(&json!([
            { "name": "u", "type": 6, "value": "80351110224678912" },
            { "name": "q", "type": 3, "value": "ğüşçö 😀", "focused": true },
            { "name": "gün", "type": 5, "value": true }
        ])));
        assert_eq!(
            i.get("u"),
            Some(&OptionValue::User(Id::new(80_351_110_224_678_912)))
        );
        assert_eq!(i.get("q"), Some(&OptionValue::String("ğüşçö 😀".into())));
        assert_eq!(i.get("gün"), Some(&OptionValue::Boolean(true)));
    }

    fn user_json(id: &str) -> Value {
        json!({ "id": id, "username": "a", "discriminator": "0000", "avatar": null, "global_name": null })
    }

    fn resolved_input(options: &Value, resolved: &Value) -> CommandInput {
        ok(input(&slash(&json!({
            "name": "warn", "options": options, "resolved": resolved
        }))))
    }

    #[test]
    fn required_and_optional() {
        let i = ok(leaf(&json!([{ "name": "days", "type": 4, "value": 3 }])));
        assert_eq!(required::<i64>(&i, "days"), Ok(3));
        assert_eq!(optional::<i64>(&i, "days"), Ok(Some(3)));
        assert_eq!(optional::<i64>(&i, "reason"), Ok(None));
        let err = required::<i64>(&i, "member").unwrap_err();
        assert_eq!(err.to_string(), "required option `member` is missing");
    }

    #[test]
    fn opt_02_wrong_type() {
        let i = ok(leaf(&json!([
            { "name": "days", "type": 3, "value": "3" },
            { "name": "n", "type": 4, "value": 2 }
        ])));
        let err = required::<i64>(&i, "days").unwrap_err();
        assert_eq!(
            err,
            OptionError::WrongType {
                option: "days".into(),
                expected: OptionType::Integer,
                found: OptionType::String
            }
        );
        assert_eq!(
            err.to_string(),
            "[OPT-02] option `days`: expected INTEGER, found STRING"
        );
        assert!(matches!(
            required::<f64>(&i, "n"),
            Err(OptionError::WrongType {
                expected: OptionType::Number,
                found: OptionType::Integer,
                ..
            })
        ));
        // `decode` itself rejects a mismatched value too.
        assert!(matches!(
            bool::decode(&OptionValue::Integer(1), &i, "b"),
            Err(OptionError::WrongType { .. })
        ));
    }

    #[test]
    fn resolved_entities() {
        let i = resolved_input(
            &json!([
                { "name": "member", "type": 6, "value": "42" },
                { "name": "m", "type": 9, "value": "42" },
                { "name": "r", "type": 9, "value": "7" },
                { "name": "x", "type": 9, "value": "8" }
            ]),
            &json!({
                "users": { "42": user_json("42") },
                "roles": { "7": {
                    "id": "7", "name": "mods", "color": 0, "colors": { "primary_color": 0 }, "hoist": false, "managed": false,
                    "mentionable": false, "permissions": "0", "position": 1, "flags": 0
                }}
            }),
        );
        let user = required::<ResolvedUser>(&i, "member").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(user.user.id.get(), 42);
        assert!(user.member.is_none());
        assert!(matches!(
            required::<Mentionable>(&i, "m"),
            Ok(Mentionable::User(_))
        ));
        assert!(matches!(
            required::<Mentionable>(&i, "r"),
            Ok(Mentionable::Role(_))
        ));
        let err = required::<Mentionable>(&i, "x").unwrap_err();
        assert_eq!(
            err,
            OptionError::Unresolved {
                option: "x".into(),
                id: 8
            }
        );
        assert_eq!(
            err.to_string(),
            "option `x`: id 8 is not in the resolved data"
        );
    }

    #[test]
    fn unresolved_and_plain_ids() {
        let i = ok(leaf(&json!([
            { "name": "u", "type": 6, "value": "42" },
            { "name": "c", "type": 7, "value": "43" },
            { "name": "r", "type": 8, "value": "44" },
            { "name": "m", "type": 9, "value": "45" },
            { "name": "a", "type": 11, "value": "46" }
        ])));
        assert!(matches!(
            required::<ResolvedUser>(&i, "u"),
            Err(OptionError::Unresolved { id: 42, .. })
        ));
        assert!(matches!(
            required::<ResolvedChannel>(&i, "c"),
            Err(OptionError::Unresolved { id: 43, .. })
        ));
        assert!(matches!(
            required::<ResolvedRole>(&i, "r"),
            Err(OptionError::Unresolved { id: 44, .. })
        ));
        assert!(matches!(
            required::<ResolvedAttachment>(&i, "a"),
            Err(OptionError::Unresolved { id: 46, .. })
        ));
        assert_eq!(required::<Id<UserMarker>>(&i, "u"), Ok(Id::new(42)));
        assert_eq!(required::<Id<ChannelMarker>>(&i, "c"), Ok(Id::new(43)));
        assert_eq!(required::<Id<RoleMarker>>(&i, "r"), Ok(Id::new(44)));
        assert_eq!(required::<Id<GenericMarker>>(&i, "m"), Ok(Id::new(45)));
        assert_eq!(required::<Id<AttachmentMarker>>(&i, "a"), Ok(Id::new(46)));
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Severity {
        Low,
    }

    impl noctis_ui::Choices for Severity {
        type Count = noctis_ui::Arity<1>;
        const ALL: &'static [Self] = &[Self::Low];
        fn label(&self) -> &'static str {
            "Low"
        }
        fn value(&self) -> &'static str {
            "low"
        }
        fn description(&self) -> Option<&'static str> {
            None
        }
        fn from_value(value: &str) -> Option<Self> {
            (value == "low").then_some(Self::Low)
        }
    }

    impl CommandOption for Severity {
        const KIND: OptionType = OptionType::String;
        fn decode(
            value: &OptionValue,
            _: &CommandInput,
            option: &str,
        ) -> Result<Self, OptionError> {
            choice(value, option)
        }
    }

    #[test]
    fn choices() {
        let i = ok(leaf(&json!([
            { "name": "a", "type": 3, "value": "low" },
            { "name": "b", "type": 3, "value": "zzz" },
            { "name": "c", "type": 4, "value": 1 }
        ])));
        assert_eq!(required::<Severity>(&i, "a"), Ok(Severity::Low));
        assert_eq!(
            required::<Severity>(&i, "b"),
            Err(OptionError::UnknownChoice {
                option: "b".into(),
                value: "zzz".into()
            })
        );
        assert!(matches!(
            required::<Severity>(&i, "c"),
            Err(OptionError::WrongType { .. })
        ));
        assert!(matches!(
            choice::<Severity>(&OptionValue::Integer(1), "c"),
            Err(OptionError::WrongType { .. })
        ));
    }

    #[test]
    fn option_value_kinds() {
        for (value, kind) in [
            (OptionValue::String(String::new()), OptionType::String),
            (OptionValue::Integer(0), OptionType::Integer),
            (OptionValue::Number(0.0), OptionType::Number),
            (OptionValue::Boolean(false), OptionType::Boolean),
            (OptionValue::User(Id::new(1)), OptionType::User),
            (OptionValue::Channel(Id::new(1)), OptionType::Channel),
            (OptionValue::Role(Id::new(1)), OptionType::Role),
            (
                OptionValue::Mentionable(Id::new(1)),
                OptionType::Mentionable,
            ),
            (OptionValue::Attachment(Id::new(1)), OptionType::Attachment),
        ] {
            assert_eq!(value.kind(), kind);
        }
    }
}
