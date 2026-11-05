//! Prefix commands emulated from slash Commands (P2, FR-14, AD-32):
//! [`PrefixPlugin`].
//!
//! Every slash Command whose `command_type` is not `"slash"` also runs from
//! a message `<prefix><Command Path> <arguments>`: the same `#[handler]`,
//! through `Dispatcher::run_command`, and every argument failure is the
//! same typed `OptionError` a failed slash decode produces. Prefix commands
//! are never coded separately.
//!
//! This crate is internal; use `noctis` (feature `prefix`) instead. Its API is
//! not stable and may change in any release.

mod guard;
mod parse;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use noctis_core::__rules::command::OptionType;
use noctis_core::events::{CommandPermissionsUpdate, EventCtx, MessageCreate};
use noctis_core::{
    AppBuilder, BuildViolation, CommandInput, CommandInvocation, DispatchError, Dispatcher,
    EventHandler, HandlerResult, Module, OptionError, OptionValue, Plugin,
};
use twilight_model::channel::Message;
use twilight_model::gateway::Intents;

use crate::guard::{Guards, Verdict};
use crate::parse::{Entry, Table};

/// The longest prefix, in Unicode scalar values.
const MAX_PREFIX_CHARS: usize = 32;

/// The default and the bounds of [`PrefixPlugin::ephemeral_ttl`].
const EPHEMERAL_TTL: Duration = Duration::from_secs(10);
const MIN_EPHEMERAL_TTL: Duration = Duration::from_secs(1);
const MAX_EPHEMERAL_TTL: Duration = Duration::from_mins(1);

/// The Module the plugin mounts its `MESSAGE_CREATE` handler in.
const MODULE: &str = "noctis-prefix";

/// Runs the App's slash Commands from messages such as `!warn <@5> 3 rude`.
///
/// `PrefixPlugin::new("!")` takes the prefix (1 to 32 Unicode scalars, no
/// whitespace, case-sensitive). Every Command the App mounts answers it,
/// unless it is declared `#[command(.., command_type = "slash")]`; a
/// `command_type = "prefix"` Command answers only it and is never
/// published. The App must request `MESSAGE_CONTENT`
/// (`.intents(Intents::MESSAGE_CONTENT)`), or the build fails with GW-05
/// `MissingIntent`.
///
/// Arguments are positional, in option declaration order. A `"…"` quote
/// (with `\"` and `\\` escapes) makes one argument of several words, and
/// the last non-ATTACHMENT option, when it is a STRING, takes the rest of
/// the line verbatim. ATTACHMENT options take the message's attachments in
/// order. A bad argument answers like a failed slash decode: a public
/// validation notice, a Command Error Sink report, and no handler run. A
/// guild-scoped Command answers only in its guild.
///
/// An ephemeral answer (`response = ephemeral`, `reply_as(.., Ephemeral)`,
/// an ephemeral follow-up) goes to the author's DM, with its edits and
/// follow-ups. When Discord refuses the DM (`50007`), it is a reply in the
/// channel deleted after [`ephemeral_ttl`](Self::ephemeral_ttl).
///
/// Messages from bots or webhooks, system messages, and messages that do
/// not name a Command Path are ignored without a request.
///
/// ```no_run
/// use noctis_core::App;
/// use noctis_prefix::PrefixPlugin;
/// use twilight_model::gateway::Intents;
///
/// let mut builder = App::builder();
/// builder
///     .intents(Intents::MESSAGE_CONTENT)
///     .plugin(PrefixPlugin::new("!"));
/// ```
#[derive(Clone, Debug)]
pub struct PrefixPlugin {
    prefix: String,
    ephemeral_ttl: Duration,
}

impl PrefixPlugin {
    /// A plugin answering messages that start with `prefix` with the App's
    /// Commands.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            ephemeral_ttl: EPHEMERAL_TTL,
        }
    }

    /// How long an ephemeral answer stays in the channel when Discord
    /// refuses the author's DM: 1 to 60 seconds (another value fails the
    /// build), 10 seconds by default.
    #[must_use]
    pub fn ephemeral_ttl(mut self, ttl: Duration) -> Self {
        self.ephemeral_ttl = ttl;
        self
    }

    /// What is wrong with the configuration (P1-P4), one message per
    /// violation.
    fn violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.prefix.is_empty() {
            out.push("prefix is empty: pass one such as \"!\"".to_owned());
        } else if self.prefix.contains(char::is_whitespace) {
            out.push(format!(
                "prefix contains whitespace: {:?} cannot be typed before a Command Path",
                self.prefix
            ));
        }
        if self.prefix.chars().count() > MAX_PREFIX_CHARS {
            out.push(format!(
                "prefix is longer than {MAX_PREFIX_CHARS} characters: shorten {:?}",
                self.prefix
            ));
        }
        if !(MIN_EPHEMERAL_TTL..=MAX_EPHEMERAL_TTL).contains(&self.ephemeral_ttl) {
            out.push(format!(
                "ephemeral_ttl is {:?}: pass 1 to 60 seconds",
                self.ephemeral_ttl
            ));
        }
        out
    }
}

/// The table of every Command Path the built App serves from messages.
fn table(prefix: &str, dispatcher: &Dispatcher) -> Table {
    let entries = dispatcher
        .__prefix_commands()
        .into_iter()
        .map(|(scope, path, handler, metadata)| {
            Entry::new(scope, path, handler.options(), metadata)
        })
        .collect();
    Table::new(prefix, entries)
}

impl Plugin for PrefixPlugin {
    fn install(&self, app: &mut AppBuilder) {
        for message in self.violations() {
            app.report(BuildViolation::plugin::<Self>(message));
        }
        let prefix: Arc<str> = Arc::from(self.prefix.as_str());
        let ttl = self.ephemeral_ttl;
        let guards = Arc::new(Guards::default());
        let updates = Arc::clone(&guards);
        let table = Arc::new(OnceLock::new());
        let handler = move |ctx: EventCtx, event: MessageCreate| {
            let table = Arc::clone(&table);
            let prefix = Arc::clone(&prefix);
            let guards = Arc::clone(&guards);
            async move {
                let table = table.get_or_init(|| self::table(&prefix, ctx.dispatcher()));
                on_message(table, &guards, ttl, &ctx, event.0).await
            }
        };
        // Discord sends it when the guild's command permission overrides
        // change: the next prefix run reads them again.
        let on_update = move |_ctx: EventCtx, event: CommandPermissionsUpdate| {
            updates.invalidate(event.0.guild_id);
            async { Ok(()) }
        };
        app.module(
            Module::new(MODULE)
                .__add(
                    EventHandler::__new::<MessageCreate, _, _>(
                        "noctis_prefix::PrefixPlugin",
                        false,
                        false,
                        handler,
                    )
                    .__with_intents(Intents::MESSAGE_CONTENT),
                )
                .__add(EventHandler::__new::<CommandPermissionsUpdate, _, _>(
                    "noctis_prefix::PrefixPlugin::permissions",
                    false,
                    false,
                    on_update,
                )),
        );
    }
}

/// Runs the Command a message names, if any.
async fn on_message(
    table: &Table,
    guards: &Guards,
    ephemeral_ttl: Duration,
    ctx: &EventCtx,
    message: Message,
) -> HandlerResult<()> {
    if parse::is_ignored(&message) {
        return Ok(());
    }
    let guild = message.guild_id.map(twilight_model::id::Id::get);
    let Some((entry, args)) = table.find(&message.content, guild) else {
        return Ok(());
    };
    if !entry.allowed(message.guild_id.is_some()) {
        tracing::debug!(
            path = entry.path(),
            "prefix command not allowed in this context"
        );
        return Ok(());
    }
    let verdict = guards.check(ctx, entry, &message).await;
    if verdict == Verdict::Ignore {
        tracing::debug!(path = entry.path(), "prefix command hidden in this channel");
        return Ok(());
    }
    let mut input = parse::bind(entry.path(), args, entry.options(), &message);
    if verdict == Verdict::Run && input.error().is_none() {
        input = check_channel_types(ctx, entry, input).await;
    }
    let mut invocation =
        CommandInvocation::new(input, message.channel_id, message.id, message.author.id)
            .guild_id(message.guild_id)
            .shard(ctx.shard())
            .ephemeral_ttl(ephemeral_ttl);
    if verdict == Verdict::Deny {
        invocation = invocation.__deny();
    }
    if let Err(err) = ctx.dispatcher().run_command(invocation).await {
        match err {
            DispatchError::UnknownCommand { path } => {
                tracing::debug!(%path, "prefix command is not mounted in this scope");
            }
            other => tracing::debug!(error = %other, "prefix command not dispatched"),
        }
    }
    Ok(())
}

/// OPT-07 for a message: each bound CHANNEL option with `channel_types`
/// must name a channel of a listed type (a slash client offers no other);
/// the first that does not marks `input` failed.
async fn check_channel_types(ctx: &EventCtx, entry: &Entry, input: CommandInput) -> CommandInput {
    for option in entry
        .options()
        .iter()
        .filter(|o| o.kind == OptionType::Channel && !o.channel_types.is_empty())
    {
        let Some(OptionValue::Channel(id)) = input.get(option.name) else {
            continue;
        };
        let kind = match ctx.cache().channel(*id).await {
            Ok(Some(channel)) => Some(channel.kind),
            _ => match ctx.http().channel(*id).await {
                Ok(response) => response.model().await.ok().map(|c| c.kind),
                Err(_) => None,
            },
        };
        let reason = match kind {
            Some(kind) if option.channel_types.contains(&u8::from(kind)) => continue,
            Some(kind) => format!(
                "[OPT-07] channel type {} is not one of the option's channel_types {:?}",
                u8::from(kind),
                option.channel_types
            ),
            None => "[OPT-07] the channel could not be read to check its type".to_owned(),
        };
        return input.with_error(OptionError::Constraint {
            option: option.name.to_owned(),
            reason,
        });
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(prefix: &str) -> PrefixPlugin {
        PrefixPlugin::new(prefix)
    }

    fn only(plugin: &PrefixPlugin) -> String {
        let violations = plugin.violations();
        assert_eq!(violations.len(), 1, "{violations:?}");
        violations[0].clone()
    }

    /// P1.
    #[test]
    fn p1_prefix_empty() {
        assert!(only(&plugin("")).starts_with("prefix is empty"));
    }

    /// P2.
    #[test]
    fn p2_prefix_whitespace() {
        for prefix in ["! ", "a b", "\u{3000}"] {
            let message = only(&plugin(prefix));
            assert!(
                message.starts_with("prefix contains whitespace"),
                "{message}"
            );
        }
    }

    /// P3.
    #[test]
    fn p3_prefix_length() {
        assert!(plugin(&"ş".repeat(32)).violations().is_empty());
        let message = only(&plugin(&"ş".repeat(33)));
        assert!(
            message.starts_with("prefix is longer than 32 characters"),
            "{message}"
        );
    }

    /// P4.
    #[test]
    fn p4_ephemeral_ttl() {
        for secs in [1, 10, 60] {
            let plugin = plugin("!").ephemeral_ttl(Duration::from_secs(secs));
            assert!(plugin.violations().is_empty(), "{secs}");
        }
        for ttl in [
            Duration::ZERO,
            Duration::from_millis(999),
            Duration::from_secs(61),
        ] {
            let message = only(&plugin("!").ephemeral_ttl(ttl));
            assert!(message.starts_with("ephemeral_ttl is"), "{message}");
        }
    }
}
