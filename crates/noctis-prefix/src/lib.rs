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

mod parse;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use noctis_core::__rules::command::OptionType;
use noctis_core::events::{EventCtx, MessageCreate};
use noctis_core::{
    AppBuilder, BuildViolation, CommandInvocation, DispatchError, Dispatcher, EventHandler,
    HandlerResult, Module, Plugin,
};
use twilight_model::channel::Message;
use twilight_model::gateway::Intents;

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
        .filter_map(|(scope, path, handler, metadata)| {
            let entry = Entry::new(scope, path, handler.options(), metadata);
            match unenforced(&entry) {
                Some(what) => {
                    tracing::debug!(
                        path,
                        command = handler.name(),
                        "not a prefix command: it declares {what}, which a message does not \
                         enforce yet"
                    );
                    None
                }
                None => Some(entry),
            }
        })
        .collect();
    Table::new(prefix, entries)
}

/// What `entry` declares that a message does not enforce yet, if anything.
fn unenforced(entry: &Entry) -> Option<&'static str> {
    if entry.metadata().default_member_permissions.is_some() {
        Some("`permissions`")
    } else if entry.metadata().nsfw == Some(true) {
        Some("`nsfw = true`")
    } else if entry
        .options()
        .iter()
        .any(|o| o.kind == OptionType::Channel && !o.channel_types.is_empty())
    {
        Some("`channel_types`")
    } else {
        None
    }
}

impl Plugin for PrefixPlugin {
    fn install(&self, app: &mut AppBuilder) {
        for message in self.violations() {
            app.report(BuildViolation::plugin::<Self>(message));
        }
        let prefix: Arc<str> = Arc::from(self.prefix.as_str());
        let ttl = self.ephemeral_ttl;
        let table = Arc::new(OnceLock::new());
        let handler = move |ctx: EventCtx, event: MessageCreate| {
            let table = Arc::clone(&table);
            let prefix = Arc::clone(&prefix);
            async move {
                let table = table.get_or_init(|| self::table(&prefix, ctx.dispatcher()));
                on_message(table, ttl, &ctx, event.0).await
            }
        };
        app.module(
            Module::new(MODULE).__add(
                EventHandler::__new::<MessageCreate, _, _>(
                    "noctis_prefix::PrefixPlugin",
                    false,
                    false,
                    handler,
                )
                .__with_intents(Intents::MESSAGE_CONTENT),
            ),
        );
    }
}

/// Runs the Command a message names, if any.
async fn on_message(
    table: &Table,
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
    let input = parse::bind(entry.path(), args, entry.options(), &message);
    let invocation =
        CommandInvocation::new(input, message.channel_id, message.id, message.author.id)
            .guild_id(message.guild_id)
            .shard(ctx.shard())
            .ephemeral_ttl(ephemeral_ttl);
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

#[cfg(test)]
mod tests {
    use noctis_core::__rules::command::{OptionDecl, Scope};
    use noctis_core::{CommandMetadata, Permissions};

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

    /// Until guards are emulated, a Command that
    /// declares them is left out of the table.
    #[test]
    fn unenforced_metadata_is_left_out() {
        static CHANNEL: [OptionDecl<'static>; 1] =
            [OptionDecl::new("where", OptionType::Channel, "d").channel_types(&[0])];
        static PLAIN_CHANNEL: [OptionDecl<'static>; 1] =
            [OptionDecl::new("where", OptionType::Channel, "d")];
        let entry = |options, metadata| Entry::new(Scope::Global, "p", options, metadata);
        let perms = CommandMetadata::new().default_member_permissions(Permissions::BAN_MEMBERS);
        assert_eq!(unenforced(&entry(&[], perms)), Some("`permissions`"));
        let nsfw = CommandMetadata::new().nsfw(true);
        assert_eq!(unenforced(&entry(&[], nsfw)), Some("`nsfw = true`"));
        assert_eq!(
            unenforced(&entry(&CHANNEL, CommandMetadata::new())),
            Some("`channel_types`")
        );
        let fine = CommandMetadata::new().nsfw(false);
        assert_eq!(unenforced(&entry(&PLAIN_CHANNEL, fine)), None);
    }
}
