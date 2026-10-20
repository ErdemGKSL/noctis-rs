//! The built-in command line (spine Conventions, FR-13): `--publish`,
//! `--publish-guild [<id>]`, `--publish-clear [--guild <id>]` and
//! `--dry-run`, parsed without clap.
//!
//! [`App::run`](crate::App::run) parses the process arguments with
//! [`NoctisArgs::parse`]; [`App::run_with_args`](crate::App::run_with_args)
//! takes them explicitly. A bot with its own flags calls
//! [`NoctisArgs::split`] first and hands the Noctis part over.

use std::ffi::OsString;

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

const PUBLISH: &str = "--publish";
const PUBLISH_GUILD: &str = "--publish-guild";
const PUBLISH_CLEAR: &str = "--publish-clear";
const GUILD: &str = "--guild";
const DRY_RUN: &str = "--dry-run";
/// The end-of-options marker: it and everything after it are never parsed.
const END: &str = "--";

/// What a run with Noctis flags does instead of starting the shards.
///
/// ```
/// use noctis_core::cli::{CliMode, NoctisArgs};
///
/// let args = NoctisArgs::parse(["--publish-clear"])?;
/// assert_eq!(args.mode(), Some(CliMode::PublishClear(None)));
/// # Ok::<(), noctis_core::cli::CliError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    /// `--publish`: publish globally (and each guild-scoped Command to its
    /// guilds), then exit.
    Publish,
    /// `--publish-guild [<id>]`: publish every Command into one guild (the
    /// dev mirror), then exit. `None` uses `AppBuilder::dev_guild`.
    PublishGuild(Option<Id<GuildMarker>>),
    /// `--publish-clear [--guild <id>]`: delete the App's global commands
    /// (or that guild's), then exit.
    PublishClear(Option<Id<GuildMarker>>),
}

impl CliMode {
    /// The flag that selects the mode.
    const fn flag(self) -> &'static str {
        match self {
            Self::Publish => PUBLISH,
            Self::PublishGuild(_) => PUBLISH_GUILD,
            Self::PublishClear(_) => PUBLISH_CLEAR,
        }
    }
}

/// The Noctis part of a command line: an optional [`CliMode`] and
/// `--dry-run`.
///
/// The default value (no mode) runs the bot. Iterating it renders the
/// canonical flags, so `NoctisArgs::parse(args.clone())` gives `args` back.
///
/// ```
/// use noctis_core::cli::{CliMode, NoctisArgs};
/// use twilight_model::id::Id;
///
/// let args = NoctisArgs::parse(["--publish-guild=42", "--dry-run"])?;
/// assert_eq!(args.mode(), Some(CliMode::PublishGuild(Some(Id::new(42)))));
/// assert!(args.dry_run());
/// assert_eq!(NoctisArgs::parse(args.clone())?, args);
/// # Ok::<(), noctis_core::cli::CliError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoctisArgs {
    mode: Option<CliMode>,
    dry_run: bool,
}

/// Why a command line was rejected. Each message names the flag and a fix.
///
/// ```
/// use noctis_core::cli::{CliError, NoctisArgs};
///
/// let err = NoctisArgs::parse(["--bogus"]).unwrap_err();
/// assert_eq!(err, CliError::UnknownFlag { flag: "--bogus".into() });
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// An argument Noctis does not know reached `parse` (or `App::run`).
    #[error(
        "unknown argument `{flag}`: Noctis accepts --publish, --publish-guild [<id>], \
         --publish-clear [--guild <id>] and --dry-run; split your own flags off first with \
         `NoctisArgs::split`"
    )]
    UnknownFlag {
        /// The argument (lossily converted when it is not UTF-8).
        flag: String,
    },
    /// A flag that needs a value got none.
    #[error("`{flag}` needs a guild id: write `{flag} <id>`")]
    MissingValue {
        /// The flag.
        flag: &'static str,
    },
    /// A flag that takes no value got one with `=`.
    #[error("`{flag}` takes no value: remove the `=…` part")]
    UnexpectedValue {
        /// The flag.
        flag: &'static str,
    },
    /// A guild id is not 1-20 ASCII digits of a non-zero `u64`.
    #[error(
        "`{flag}` got `{value}`, which is not a guild id: use the guild's numeric id \
         (1-20 digits, not 0)"
    )]
    InvalidGuildId {
        /// The flag the value belongs to.
        flag: &'static str,
        /// The rejected value.
        value: String,
    },
    /// A flag was given twice.
    #[error("`{flag}` is given twice: keep one")]
    DuplicateFlag {
        /// The flag.
        flag: &'static str,
    },
    /// Two mode flags were given.
    #[error("`{first}` and `{second}` cannot be combined: run one mode at a time")]
    ConflictingModes {
        /// The first mode flag.
        first: &'static str,
        /// The second mode flag.
        second: &'static str,
    },
    /// `--dry-run` without a publish flag.
    #[error("`--dry-run` needs a publish flag: add --publish, --publish-guild or --publish-clear")]
    DryRunWithoutPublish,
    /// `--guild` without `--publish-clear`.
    #[error(
        "`--guild` only applies to --publish-clear: use `--publish-guild <id>` to publish to \
         one guild"
    )]
    GuildWithoutClear,
    /// `--publish-guild` without an id and no dev guild set.
    #[error(
        "`--publish-guild` has no guild id and no dev guild is set: pass \
         `--publish-guild <id>`, call `AppBuilder::dev_guild` or set NOCTIS_DEV_GUILD"
    )]
    MissingDevGuild,
}

/// The guild id rule, shared by the command line and `NOCTIS_DEV_GUILD`:
/// 1-20 ASCII digits of a non-zero `u64`.
pub(crate) fn parse_guild_id(raw: &str) -> Option<Id<GuildMarker>> {
    let digits = (1..=20).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_digit());
    digits
        .then(|| raw.parse::<u64>().ok())
        .flatten()
        .and_then(Id::new_checked)
}

/// [`parse_guild_id`] for a flag's value.
fn guild_id(flag: &'static str, value: &str) -> Result<Id<GuildMarker>, CliError> {
    parse_guild_id(value).ok_or_else(|| CliError::InvalidGuildId {
        flag,
        value: value.to_owned(),
    })
}

/// The parse state between arguments.
#[derive(Default)]
struct Parser {
    mode: Option<CliMode>,
    dry_run: bool,
    guild: Option<Id<GuildMarker>>,
}

impl Parser {
    fn set_mode(&mut self, mode: CliMode) -> Result<(), CliError> {
        match self.mode {
            Some(first) if first.flag() == mode.flag() => {
                Err(CliError::DuplicateFlag { flag: mode.flag() })
            }
            Some(first) => Err(CliError::ConflictingModes {
                first: first.flag(),
                second: mode.flag(),
            }),
            None => {
                self.mode = Some(mode);
                Ok(())
            }
        }
    }

    /// Consumes `arg` (and maybe the next argument) when it is a Noctis
    /// flag; `Ok(false)` when it is not.
    fn flag(
        &mut self,
        arg: &str,
        next: &mut std::iter::Peekable<impl Iterator<Item = OsString>>,
    ) -> Result<bool, CliError> {
        let (name, value) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value)),
            _ => (arg, None),
        };
        let no_value = |flag: &'static str| match value {
            Some(_) => Err(CliError::UnexpectedValue { flag }),
            None => Ok(()),
        };
        match name {
            PUBLISH => {
                no_value(PUBLISH)?;
                self.set_mode(CliMode::Publish)?;
            }
            PUBLISH_CLEAR => {
                no_value(PUBLISH_CLEAR)?;
                self.set_mode(CliMode::PublishClear(None))?;
            }
            DRY_RUN => {
                no_value(DRY_RUN)?;
                if self.dry_run {
                    return Err(CliError::DuplicateFlag { flag: DRY_RUN });
                }
                self.dry_run = true;
            }
            PUBLISH_GUILD => {
                let id = match value {
                    Some(value) => Some(guild_id(PUBLISH_GUILD, value)?),
                    None => match take_value(next) {
                        Some(value) => Some(guild_id(PUBLISH_GUILD, &value)?),
                        None => None,
                    },
                };
                self.set_mode(CliMode::PublishGuild(id))?;
            }
            GUILD => {
                let value = match value {
                    Some(value) => value.to_owned(),
                    None => take_value(next).ok_or(CliError::MissingValue { flag: GUILD })?,
                };
                let id = guild_id(GUILD, &value)?;
                if self.guild.replace(id).is_some() {
                    return Err(CliError::DuplicateFlag { flag: GUILD });
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn finish(self) -> Result<NoctisArgs, CliError> {
        if self.dry_run && self.mode.is_none() {
            return Err(CliError::DryRunWithoutPublish);
        }
        let mode = match (self.mode, self.guild) {
            (Some(CliMode::PublishClear(_)), guild) => Some(CliMode::PublishClear(guild)),
            (_, Some(_)) => return Err(CliError::GuildWithoutClear),
            (mode, None) => mode,
        };
        Ok(NoctisArgs {
            mode,
            dry_run: self.dry_run,
        })
    }
}

/// The next argument as a flag value: taken only when it is not a flag
/// (does not start with `-`). A non-UTF-8 value is taken lossily, so it
/// fails the id rule.
fn take_value(next: &mut std::iter::Peekable<impl Iterator<Item = OsString>>) -> Option<String> {
    let peeked = next.peek()?;
    let starts_with_dash = peeked.as_encoded_bytes().first() == Some(&b'-');
    if starts_with_dash {
        return None;
    }
    next.next()
        .map(|value| value.to_string_lossy().into_owned())
}

impl NoctisArgs {
    /// Parses a command line that holds only Noctis flags (without the
    /// program name), as [`App::run_with_args`](crate::App::run_with_args)
    /// does.
    ///
    /// `--publish-guild` takes the next argument as its id only when it
    /// does not start with `-`; `--guild <id>` belongs to
    /// `--publish-clear`. The two value flags also accept `--flag=value`.
    ///
    /// # Errors
    ///
    /// The first [`CliError`]: any other argument (including `--` and a
    /// non-UTF-8 one) is [`CliError::UnknownFlag`]; then the flag checks.
    ///
    /// ```
    /// use noctis_core::cli::{CliError, CliMode, NoctisArgs};
    ///
    /// assert_eq!(NoctisArgs::parse(["--publish"])?.mode(), Some(CliMode::Publish));
    /// assert_eq!(
    ///     NoctisArgs::parse(["--dry-run"]),
    ///     Err(CliError::DryRunWithoutPublish)
    /// );
    /// # Ok::<(), CliError>(())
    /// ```
    pub fn parse<I, A>(args: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        let (args, rest) = Self::split_inner(args)?;
        match rest.into_iter().next() {
            Some(first) => Err(CliError::UnknownFlag {
                flag: first.to_string_lossy().into_owned(),
            }),
            None => Ok(args),
        }
    }

    /// Splits a full command line (program name included) into the Noctis
    /// flags and everything else, which keeps its order untouched: the
    /// program name, your own flags, positional arguments, non-UTF-8
    /// arguments, and `--` with everything after it.
    ///
    /// Before `--`, the flag names `--publish`, `--publish-guild`,
    /// `--publish-clear`, `--guild` and `--dry-run` are reserved by Noctis
    /// and always claimed (rename a flag of your own that uses one), and
    /// `--publish-guild` also consumes the next argument when it does not
    /// start with `-`.
    ///
    /// # Errors
    ///
    /// A malformed Noctis flag (for example `--publish-guild 0` or two
    /// modes) is a [`CliError`].
    ///
    /// ```
    /// use noctis_core::cli::{CliMode, NoctisArgs};
    /// use twilight_model::id::Id;
    ///
    /// let (noctis, mine) =
    ///     NoctisArgs::split(["bot", "--verbose", "--publish-guild", "5", "--", "--dry-run"])?;
    /// assert_eq!(noctis.mode(), Some(CliMode::PublishGuild(Some(Id::new(5)))));
    /// assert!(!noctis.dry_run());
    /// assert_eq!(mine, ["bot", "--verbose", "--", "--dry-run"]);
    /// # Ok::<(), noctis_core::cli::CliError>(())
    /// ```
    pub fn split<I, A>(args: I) -> Result<(Self, Vec<OsString>), CliError>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        Self::split_inner(args)
    }

    fn split_inner<I, A>(args: I) -> Result<(Self, Vec<OsString>), CliError>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        let mut args = args.into_iter().map(Into::into).peekable();
        let mut parser = Parser::default();
        let mut rest = Vec::new();
        while let Some(arg) = args.next() {
            let Some(text) = arg.to_str() else {
                rest.push(arg);
                continue;
            };
            if text == END {
                rest.push(arg);
                rest.extend(args.by_ref());
                break;
            }
            if !parser.flag(text, &mut args)? {
                rest.push(arg);
            }
        }
        Ok((parser.finish()?, rest))
    }

    /// The mode, or `None` to run the bot.
    pub fn mode(&self) -> Option<CliMode> {
        self.mode
    }

    /// Whether `--dry-run` was given (only with a mode).
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
}

impl IntoIterator for NoctisArgs {
    type Item = OsString;
    type IntoIter = std::vec::IntoIter<OsString>;

    /// The canonical flags: the mode flag (with its id), then `--dry-run`.
    fn into_iter(self) -> Self::IntoIter {
        let mut out: Vec<OsString> = Vec::new();
        match self.mode {
            None => {}
            Some(CliMode::Publish) => out.push(PUBLISH.into()),
            Some(CliMode::PublishGuild(id)) => {
                out.push(PUBLISH_GUILD.into());
                out.extend(id.map(|id| id.get().to_string().into()));
            }
            Some(CliMode::PublishClear(id)) => {
                out.push(PUBLISH_CLEAR.into());
                if let Some(id) = id {
                    out.push(GUILD.into());
                    out.push(id.get().to_string().into());
                }
            }
        }
        if self.dry_run {
            out.push(DRY_RUN.into());
        }
        out.into_iter()
    }
}

#[cfg(test)]
mod tests;
