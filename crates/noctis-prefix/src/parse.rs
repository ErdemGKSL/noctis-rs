//! The pure half of the plugin: which messages are ignored, which opted-in
//! Command Path a message names ([`Table::find`]), and how its arguments
//! bind to the Command's options ([`bind`]).
//!
//! Grammar (D3): arguments are positional, in declaration order, separated
//! by any run of `char::is_whitespace`. A token starting with `"` is quoted
//! up to the next unescaped `"`, with `\"` and `\\` as escapes (any other
//! backslash is literal) and whitespace or the end right after it.
//! ATTACHMENT options take the message's attachments in order and consume
//! no text. The last non-ATTACHMENT option, when it is a STRING, takes the
//! trimmed remainder verbatim, unquoted only when the remainder is exactly
//! one quoted token. Every failure is an [`OptionError`]; the first one in
//! declaration order is kept.

use std::borrow::Cow;
use std::collections::HashMap;

use noctis_core::__rules::command::{
    Choice, ChoiceValue, InteractionContext, NumericValue, OptionDecl, OptionType,
};
use noctis_core::__rules::len::discord_len;
use noctis_core::{CommandInput, OptionError, OptionValue};
use twilight_model::application::interaction::InteractionDataResolved;
use twilight_model::channel::message::{Mention, MessageType};
use twilight_model::channel::{Attachment, Message};
use twilight_model::id::Id;
use twilight_model::id::marker::{GenericMarker, UserMarker};
use twilight_model::user::User;

use crate::Opted;

/// The largest INTEGER magnitude Discord accepts (2^53 − 1, OPT-03).
const INTEGER_MAX: u64 = 9_007_199_254_740_991;
/// The largest NUMBER magnitude Discord accepts (2^53, OPT-04).
const NUMBER_MAX: f64 = 9_007_199_254_740_992.0;
/// The most decimal digits of a snowflake (`u64::MAX` has 20).
const SNOWFLAKE_DIGITS: usize = 20;

/// Whether the plugin ignores `message` without looking at its content:
/// bot and webhook authors, and every kind but `Regular` and `Reply`.
pub(crate) fn is_ignored(message: &Message) -> bool {
    message.author.bot
        || message.webhook_id.is_some()
        || !matches!(message.kind, MessageType::Regular | MessageType::Reply)
}

/// The prefix and every opted-in Command Path, longest first.
#[derive(Debug)]
pub(crate) struct Table {
    prefix: String,
    entries: Vec<Entry>,
}

/// One opted-in Command Path.
#[derive(Debug)]
pub(crate) struct Entry {
    path: &'static str,
    words: Vec<&'static str>,
    options: &'static [OptionDecl<'static>],
    contexts: Option<&'static [InteractionContext]>,
}

impl Entry {
    /// The Command Path, words joined by one ASCII space.
    pub(crate) fn path(&self) -> &'static str {
        self.path
    }

    /// The Command's option declarations.
    pub(crate) fn options(&self) -> &'static [OptionDecl<'static>] {
        self.options
    }

    /// Whether the Command's `contexts` allow a guild message (`in_guild`)
    /// or a DM; no `contexts` allows both.
    pub(crate) fn allowed(&self, in_guild: bool) -> bool {
        let place = if in_guild {
            InteractionContext::Guild
        } else {
            InteractionContext::BotDm
        };
        self.contexts
            .is_none_or(|contexts| contexts.contains(&place))
    }
}

impl Table {
    pub(crate) fn new(prefix: &str, commands: &[Opted]) -> Self {
        let mut entries: Vec<Entry> = commands
            .iter()
            .flat_map(|opted| {
                opted.paths.iter().map(|&path| Entry {
                    path,
                    words: path.split(' ').collect(),
                    options: opted.options,
                    contexts: opted.metadata.contexts,
                })
            })
            .collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.words.len()));
        Self {
            prefix: prefix.to_owned(),
            entries,
        }
    }

    /// The entry `content` names and its argument text: the prefix, then
    /// directly the path words (case-insensitive, any whitespace run
    /// between them), then whitespace or the end. Allocates nothing for a
    /// message without the prefix.
    pub(crate) fn find<'c>(&self, content: &'c str) -> Option<(&Entry, &'c str)> {
        if self.prefix.is_empty() {
            return None;
        }
        let rest = content.strip_prefix(self.prefix.as_str())?;
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return None;
        }
        self.entries
            .iter()
            .find_map(|entry| match_words(rest, &entry.words).map(|args| (entry, args)))
    }
}

/// The text after `words` at the start of `text`, if they match.
fn match_words<'c>(text: &'c str, words: &[&str]) -> Option<&'c str> {
    let mut rest = text;
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            let trimmed = rest.trim_start();
            if trimmed.len() == rest.len() {
                return None;
            }
            rest = trimmed;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if !lowercase_eq(&rest[..end], word) {
            return None;
        }
        rest = &rest[end..];
    }
    Some(rest)
}

/// `typed.to_lowercase() == word`, without allocating unless `typed` holds
/// a capital sigma (whose lowercase depends on its position).
fn lowercase_eq(typed: &str, word: &str) -> bool {
    if typed.contains('Σ') {
        return typed.to_lowercase() == word;
    }
    typed.chars().flat_map(char::to_lowercase).eq(word.chars())
}

/// Binds `args` and the message's attachments to `options`: the input of
/// the Command at `path`, marked failed with the first [`OptionError`].
pub(crate) fn bind(
    path: &str,
    args: &str,
    options: &[OptionDecl<'_>],
    message: &Message,
) -> CommandInput {
    let mut binder = Binder {
        message,
        values: Vec::new(),
        resolved: InteractionDataResolved {
            attachments: HashMap::new(),
            channels: HashMap::new(),
            members: HashMap::new(),
            messages: HashMap::new(),
            roles: HashMap::new(),
            users: HashMap::new(),
        },
    };
    let outcome = binder.bind(args, options);
    let Binder {
        values, resolved, ..
    } = binder;
    let mut input = values
        .into_iter()
        .fold(CommandInput::new(path), |input, (name, value)| {
            input.with_option(name, value)
        })
        .with_resolved(resolved);
    if let Err(error) = outcome {
        input = input.with_error(error);
    }
    input
}

struct Binder<'m> {
    message: &'m Message,
    values: Vec<(String, OptionValue)>,
    resolved: InteractionDataResolved,
}

impl Binder<'_> {
    fn bind(&mut self, args: &str, options: &[OptionDecl<'_>]) -> Result<(), OptionError> {
        let rest_of_line = options
            .iter()
            .rposition(|o| o.kind != OptionType::Attachment)
            .filter(|&index| options[index].kind == OptionType::String);
        let mut cursor = Cursor { rest: args };
        let mut attachments = self.message.attachments.iter();
        for (index, decl) in options.iter().enumerate() {
            let value = if decl.kind == OptionType::Attachment {
                attachments
                    .next()
                    .map(|a| self.attachment(decl, a))
                    .transpose()?
            } else if rest_of_line == Some(index) {
                cursor
                    .take_rest()
                    .map(|text| string(decl, &text))
                    .transpose()?
            } else {
                match cursor.next_token()? {
                    Some(token) => Some(self.value(decl, &token)?),
                    None => None,
                }
            };
            match value {
                Some(value) => self.values.push((decl.name.to_owned(), value)),
                None if decl.required => {
                    return Err(OptionError::Missing {
                        option: decl.name.to_owned(),
                    });
                }
                None => {}
            }
        }
        match cursor.next_token()? {
            Some(extra) => Err(OptionError::Malformed {
                reason: format!("unexpected argument `{extra}`"),
            }),
            None => Ok(()),
        }
    }

    /// One text argument of a non-ATTACHMENT option.
    fn value(&mut self, decl: &OptionDecl<'_>, token: &str) -> Result<OptionValue, OptionError> {
        let wrong = || wrong_type(decl, OptionType::String);
        Ok(match decl.kind {
            OptionType::String => return string(decl, token),
            OptionType::Integer => return integer(decl, token),
            OptionType::Number => return number(decl, token),
            OptionType::Boolean => OptionValue::Boolean(boolean(token).ok_or_else(wrong)?),
            OptionType::User => match mention(token) {
                Some(Mentioned::User(id) | Mentioned::Raw(id)) => {
                    let id = id.cast();
                    self.resolve_user(id);
                    OptionValue::User(id)
                }
                _ => return Err(wrong()),
            },
            OptionType::Role => match mention(token) {
                Some(Mentioned::Role(id) | Mentioned::Raw(id)) => OptionValue::Role(id.cast()),
                _ => return Err(wrong()),
            },
            OptionType::Channel => match mention(token) {
                Some(Mentioned::Channel(id) | Mentioned::Raw(id)) => {
                    OptionValue::Channel(id.cast())
                }
                _ => return Err(wrong()),
            },
            OptionType::Mentionable => match mention(token) {
                Some(Mentioned::User(id) | Mentioned::Raw(id)) => {
                    self.resolve_user(id.cast());
                    OptionValue::Mentionable(id)
                }
                Some(Mentioned::Role(id)) => OptionValue::Mentionable(id),
                _ => return Err(wrong()),
            },
            // Bound from the message's attachments, never from text.
            OptionType::Attachment => return Err(wrong()),
        })
    }

    /// Puts the user `id` into the resolved data when the message names it:
    /// its author, or one of its mentions.
    fn resolve_user(&mut self, id: Id<UserMarker>) {
        let author = &self.message.author;
        let user = if author.id == id {
            author.clone()
        } else if let Some(mention) = self.message.mentions.iter().find(|m| m.id == id) {
            user_of(mention)
        } else {
            return;
        };
        self.resolved.users.insert(id, user);
    }

    /// An attachment of an ATTACHMENT option, checked against its
    /// `file_types` (OPT-13).
    fn attachment(
        &mut self,
        decl: &OptionDecl<'_>,
        attachment: &Attachment,
    ) -> Result<OptionValue, OptionError> {
        if !decl.file_types.is_empty()
            && !decl
                .file_types
                .iter()
                .any(|file_type| file_type_matches(file_type, attachment))
        {
            return Err(constraint(
                decl,
                format!(
                    "[OPT-13] `{}` is not one of the file types {}",
                    attachment.filename,
                    decl.file_types.join(", ")
                ),
            ));
        }
        self.resolved
            .attachments
            .insert(attachment.id, attachment.clone());
        Ok(OptionValue::Attachment(attachment.id))
    }
}

/// A user from a message mention; fields a mention lacks are unset.
fn user_of(mention: &Mention) -> User {
    User {
        accent_color: None,
        avatar: mention.avatar,
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: mention.bot,
        discriminator: mention.discriminator,
        email: None,
        flags: None,
        global_name: None,
        id: mention.id,
        locale: None,
        mfa_enabled: None,
        name: mention.name.clone(),
        premium_type: None,
        primary_guild: None,
        public_flags: Some(mention.public_flags),
        system: None,
        verified: None,
    }
}

/// Whether `attachment` passes one OPT-13 `file_types` entry: a dot entry
/// is a case-insensitive filename suffix, `image`/`video`/`audio` a
/// content-type family.
fn file_type_matches(file_type: &str, attachment: &Attachment) -> bool {
    if file_type.starts_with('.') {
        let name = attachment.filename.as_bytes();
        return name.len() >= file_type.len()
            && name[name.len() - file_type.len()..].eq_ignore_ascii_case(file_type.as_bytes());
    }
    attachment.content_type.as_deref().is_some_and(|content| {
        content
            .split_once('/')
            .is_some_and(|(family, _)| family.eq_ignore_ascii_case(file_type))
    })
}

/// The argument text not consumed yet.
struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    /// The next token, unquoted.
    ///
    /// # Errors
    ///
    /// [`OptionError::Malformed`] for an unclosed quote or text right after
    /// a closing quote.
    fn next_token(&mut self) -> Result<Option<Cow<'a, str>>, OptionError> {
        let text = self.rest.trim_start();
        if text.is_empty() {
            self.rest = text;
            return Ok(None);
        }
        if let Some(body) = text.strip_prefix('"') {
            let (value, after) = quoted(body)?;
            if let Some(next) = after.chars().next()
                && !next.is_whitespace()
            {
                return Err(OptionError::Malformed {
                    reason: format!("unexpected `{next}` after closing quote"),
                });
            }
            self.rest = after;
            return Ok(Some(Cow::Owned(value)));
        }
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        self.rest = &text[end..];
        Ok(Some(Cow::Borrowed(&text[..end])))
    }

    /// The trimmed remainder (`None` when empty), unquoted when it is
    /// exactly one quoted token.
    fn take_rest(&mut self) -> Option<Cow<'a, str>> {
        let text = self.rest.trim();
        self.rest = "";
        if text.is_empty() {
            return None;
        }
        if let Some(body) = text.strip_prefix('"')
            && let Ok((value, "")) = quoted(body)
        {
            return Some(Cow::Owned(value));
        }
        Some(Cow::Borrowed(text))
    }
}

/// The unescaped text of a quoted token whose opening quote is already
/// consumed, and the text after its closing quote.
fn quoted(body: &str) -> Result<(String, &str), OptionError> {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &body[at + 1..])),
            '\\' => match chars.clone().next() {
                Some((_, escaped @ ('"' | '\\'))) => {
                    chars.next();
                    out.push(escaped);
                }
                _ => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    Err(OptionError::Malformed {
        reason: "unclosed quote in the arguments".to_owned(),
    })
}

fn wrong_type(decl: &OptionDecl<'_>, found: OptionType) -> OptionError {
    OptionError::WrongType {
        option: decl.name.to_owned(),
        expected: decl.kind,
        found,
    }
}

fn constraint(decl: &OptionDecl<'_>, reason: String) -> OptionError {
    OptionError::Constraint {
        option: decl.name.to_owned(),
        reason,
    }
}

fn unknown_choice(decl: &OptionDecl<'_>, value: &str) -> OptionError {
    OptionError::UnknownChoice {
        option: decl.name.to_owned(),
        value: value.to_owned(),
    }
}

/// Whether `choice` is named `text` (ASCII case-insensitive, default or
/// localized name).
fn named(choice: &Choice<'_>, text: &str) -> bool {
    choice.name.default.eq_ignore_ascii_case(text)
        || choice
            .name
            .localized
            .iter()
            .any(|name| name.eq_ignore_ascii_case(text))
}

/// The numeric value of the choice named `text`, if any.
fn numeric_choice(decl: &OptionDecl<'_>, text: &str) -> Option<NumericValue> {
    decl.choices
        .iter()
        .find(|choice| named(choice, text))
        .and_then(|choice| match choice.value {
            ChoiceValue::Int(v) => Some(NumericValue::Int(v)),
            ChoiceValue::Float(v) => Some(NumericValue::Float(v)),
            ChoiceValue::Str(_) => None,
        })
}

/// A STRING value: a choice by value (exact) or name, then OPT-06.
fn string(decl: &OptionDecl<'_>, text: &str) -> Result<OptionValue, OptionError> {
    let value = if decl.choices.is_empty() {
        text.to_owned()
    } else {
        let by_value = decl.choices.iter().find_map(|choice| match choice.value {
            ChoiceValue::Str(v) if v == text => Some(v),
            _ => None,
        });
        let by_name = || {
            decl.choices.iter().find_map(|choice| match choice.value {
                ChoiceValue::Str(v) if named(choice, text) => Some(v),
                _ => None,
            })
        };
        by_value
            .or_else(by_name)
            .ok_or_else(|| unknown_choice(decl, text))?
            .to_owned()
    };
    let len = discord_len(&value);
    if let Some(min) = decl.min_length
        && len < min
    {
        return Err(constraint(
            decl,
            format!("[OPT-06] length {len} is below min_length {min}"),
        ));
    }
    if let Some(max) = decl.max_length
        && len > max
    {
        return Err(constraint(
            decl,
            format!("[OPT-06] length {len} is above max_length {max}"),
        ));
    }
    Ok(OptionValue::String(value))
}

/// `[+-]?[0-9]+`, ASCII only.
fn is_integer(token: &str) -> bool {
    let digits = token.strip_prefix(['+', '-']).unwrap_or(token);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// `[+-]?(digits(.digits?)?|.digits)([eE][+-]?digits)?`, ASCII only.
fn is_number(token: &str) -> bool {
    let bytes = token.as_bytes();
    let digits_from = |at: usize| {
        bytes
            .get(at..)
            .map_or(0, |b| b.iter().take_while(|b| b.is_ascii_digit()).count())
    };
    let mut at = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int = digits_from(at);
    at += int;
    let mut frac = 0;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        frac = digits_from(at);
        at += frac;
    }
    if int + frac == 0 {
        return false;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exp = digits_from(at);
        if exp == 0 {
            return false;
        }
        at += exp;
    }
    at == bytes.len()
}

/// An INTEGER value: a choice by name, else `[+-]?digits` in `i64`, then
/// OPT-03, the choices and OPT-05.
fn integer(decl: &OptionDecl<'_>, token: &str) -> Result<OptionValue, OptionError> {
    let value = match numeric_choice(decl, token) {
        Some(NumericValue::Int(v)) => v,
        _ if is_integer(token) => token.parse::<i64>().map_err(|_| OptionError::OutOfRange {
            option: decl.name.to_owned(),
            value: token.to_owned(),
        })?,
        _ if is_number(token) => return Err(wrong_type(decl, OptionType::Number)),
        _ => return Err(wrong_type(decl, OptionType::String)),
    };
    if value.unsigned_abs() > INTEGER_MAX {
        return Err(constraint(
            decl,
            format!("[OPT-03] {value} is outside -{INTEGER_MAX}..={INTEGER_MAX}"),
        ));
    }
    if !decl.choices.is_empty()
        && !decl
            .choices
            .iter()
            .any(|c| matches!(c.value, ChoiceValue::Int(v) if v == value))
    {
        return Err(unknown_choice(decl, token));
    }
    bounds(decl, NumericValue::Int(value))?;
    Ok(OptionValue::Integer(value))
}

/// A NUMBER value: a choice by name, else a decimal number, then OPT-04,
/// the choices and OPT-05.
#[expect(
    clippy::float_cmp,
    reason = "a NUMBER choice matches only its exact declared value, as on Discord"
)]
fn number(decl: &OptionDecl<'_>, token: &str) -> Result<OptionValue, OptionError> {
    let value = match numeric_choice(decl, token) {
        Some(choice) => numeric(choice),
        _ if is_number(token) => token
            .parse::<f64>()
            .map_err(|_| wrong_type(decl, OptionType::String))?,
        _ => return Err(wrong_type(decl, OptionType::String)),
    };
    if !value.is_finite() || value.abs() > NUMBER_MAX {
        return Err(constraint(
            decl,
            format!("[OPT-04] {token} is outside -{NUMBER_MAX}..={NUMBER_MAX}"),
        ));
    }
    let is_choice = |c: &ChoiceValue<'_>| match *c {
        ChoiceValue::Float(v) => v == value,
        ChoiceValue::Int(v) => as_f64(v) == value,
        ChoiceValue::Str(_) => false,
    };
    if !decl.choices.is_empty() && !decl.choices.iter().any(|c| is_choice(&c.value)) {
        return Err(unknown_choice(decl, token));
    }
    bounds(decl, NumericValue::Float(value))?;
    Ok(OptionValue::Number(value))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "values and bounds are within ±2^53 here, where the cast is exact"
)]
fn as_f64(value: i64) -> f64 {
    value as f64
}

/// OPT-05: `value` against `min_value` and `max_value`.
fn bounds(decl: &OptionDecl<'_>, value: NumericValue) -> Result<(), OptionError> {
    let below = |bound: NumericValue| match (value, bound) {
        (NumericValue::Int(v), NumericValue::Int(b)) => v < b,
        _ => numeric(value) < numeric(bound),
    };
    let above = |bound: NumericValue| match (value, bound) {
        (NumericValue::Int(v), NumericValue::Int(b)) => v > b,
        _ => numeric(value) > numeric(bound),
    };
    if let Some(min) = decl.min_value
        && below(min)
    {
        return Err(constraint(
            decl,
            format!("[OPT-05] {value} is below min_value {min}"),
        ));
    }
    if let Some(max) = decl.max_value
        && above(max)
    {
        return Err(constraint(
            decl,
            format!("[OPT-05] {value} is above max_value {max}"),
        ));
    }
    Ok(())
}

fn numeric(value: NumericValue) -> f64 {
    match value {
        NumericValue::Int(v) => as_f64(v),
        NumericValue::Float(v) => v,
    }
}

/// `true`/`yes`/`on` or `false`/`no`/`off`, ASCII case-insensitive.
fn boolean(token: &str) -> Option<bool> {
    let is = |words: [&str; 3]| words.iter().any(|w| w.eq_ignore_ascii_case(token));
    if is(["true", "yes", "on"]) {
        Some(true)
    } else if is(["false", "no", "off"]) {
        Some(false)
    } else {
        None
    }
}

/// What an id argument names.
enum Mentioned {
    /// `<@id>` or `<@!id>`.
    User(Id<GenericMarker>),
    /// `<@&id>`.
    Role(Id<GenericMarker>),
    /// `<#id>`.
    Channel(Id<GenericMarker>),
    /// A bare id.
    Raw(Id<GenericMarker>),
}

/// The mention or bare snowflake `token` is, if any.
fn mention(token: &str) -> Option<Mentioned> {
    if let Some(inner) = token.strip_prefix("<@").and_then(|t| t.strip_suffix('>')) {
        return Some(match inner.strip_prefix('&') {
            Some(role) => Mentioned::Role(snowflake(role)?),
            None => Mentioned::User(snowflake(inner.strip_prefix('!').unwrap_or(inner))?),
        });
    }
    if let Some(inner) = token.strip_prefix("<#").and_then(|t| t.strip_suffix('>')) {
        return Some(Mentioned::Channel(snowflake(inner)?));
    }
    snowflake(token).map(Mentioned::Raw)
}

/// A non-zero snowflake of 1 to 20 ASCII digits.
fn snowflake(digits: &str) -> Option<Id<GenericMarker>> {
    if digits.is_empty()
        || digits.len() > SNOWFLAKE_DIGITS
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Id::new_checked(digits.parse().ok()?)
}

#[cfg(test)]
mod tests;
