//! Opt-in prefix commands (P2, FR-14): [`PrefixPlugin`].
//!
//! A bot opts slash Commands in by type; a message `<prefix><Command Path>
//! <arguments>` then runs the same `#[handler]` through
//! `Dispatcher::run_command`, and every argument failure is the same typed
//! `OptionError` a failed slash decode produces.
//!
//! This crate is internal; use `noctis` (feature `prefix`) instead. Its API is
//! not stable and may change in any release.

mod parse;

use std::any::TypeId;
use std::sync::Arc;

use noctis_core::__rules::command::{OptionDecl, OptionType};
use noctis_core::events::{EventCtx, MessageCreate};
use noctis_core::{
    __SlashCommand, AppBuilder, BuildViolation, CommandInvocation, CommandMetadata, DispatchError,
    EventHandler, HandlerResult, Module, Plugin,
};
use twilight_model::channel::Message;
use twilight_model::gateway::Intents;

use crate::parse::Table;

/// The longest prefix, in Unicode scalar values.
const MAX_PREFIX_CHARS: usize = 32;

/// The Module the plugin mounts its `MESSAGE_CREATE` handler in.
const MODULE: &str = "noctis-prefix";

/// Runs opted-in slash Commands from messages such as `!warn <@5> 3 rude`.
///
/// `PrefixPlugin::new("!")` takes the prefix (1 to 32 Unicode scalars, no
/// whitespace, case-sensitive); [`command`](Self::command) opts a
/// `#[derive(SlashCommand)]` struct in by type. The struct must also be
/// mounted in a Module, and the App must request `MESSAGE_CONTENT`
/// (`.intents(Intents::MESSAGE_CONTENT)`), or the build fails with GW-05
/// `MissingIntent`.
///
/// Arguments are positional, in option declaration order. A `"…"` quote
/// (with `\"` and `\\` escapes) makes one argument of several words, and
/// the last non-ATTACHMENT option, when it is a STRING, takes the rest of
/// the line verbatim. ATTACHMENT options take the message's attachments in
/// order. A bad argument answers like a failed slash decode: a public
/// validation notice, a Command Error Sink report, and no handler run.
///
/// Messages from bots or webhooks, system messages, and messages that do
/// not name an opted-in Command Path are ignored without a request.
///
/// ```no_run
/// use noctis_core::App;
/// use noctis_prefix::PrefixPlugin;
/// use twilight_model::gateway::Intents;
/// # use noctis_core::__SlashCommand;
/// # fn run<Warn: __SlashCommand>() {
///
/// let mut builder = App::builder();
/// builder
///     .intents(Intents::MESSAGE_CONTENT)
///     .plugin(PrefixPlugin::new("!").command::<Warn>());
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct PrefixPlugin {
    prefix: String,
    commands: Vec<Opted>,
}

/// One opted-in Command: what [`PrefixPlugin::command`] reads from its type.
#[derive(Clone, Copy, Debug)]
struct Opted {
    type_id: TypeId,
    /// `__Kind::NAME`: the struct's path.
    name: &'static str,
    paths: &'static [&'static str],
    options: &'static [OptionDecl<'static>],
    metadata: CommandMetadata,
}

impl PrefixPlugin {
    /// A plugin answering messages that start with `prefix`; opt Commands
    /// in with [`command`](Self::command).
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            commands: Vec::new(),
        }
    }

    /// Opts the slash Command `T` in: each of its Command Paths can be
    /// typed after the prefix. Opting the same type in twice is a no-op.
    ///
    /// A Command that declares `permissions`, `nsfw = true` or a CHANNEL
    /// option with `channel_types` cannot be opted in (a build violation),
    /// because a message carries no data to enforce them.
    #[must_use]
    pub fn command<T: __SlashCommand>(self) -> Self {
        self.opt(Opted {
            type_id: TypeId::of::<T>(),
            name: T::NAME,
            paths: T::PATHS,
            options: T::OPTIONS,
            metadata: T::METADATA,
        })
    }

    fn opt(mut self, opted: Opted) -> Self {
        if self.commands.iter().all(|c| c.type_id != opted.type_id) {
            self.commands.push(opted);
        }
        self
    }

    /// What is wrong with the configuration (P1-P6), one message per
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
        if self.commands.is_empty() {
            out.push("no Command is opted in: add `.command::<T>()`".to_owned());
        }
        for opted in &self.commands {
            let refuse = |what: String| {
                format!(
                    "`{}` cannot be a prefix command: {what}, which a message cannot enforce",
                    opted.name
                )
            };
            if opted.metadata.default_member_permissions.is_some() {
                out.push(refuse("it declares `permissions`".to_owned()));
            }
            if opted.metadata.nsfw == Some(true) {
                out.push(refuse("it declares `nsfw = true`".to_owned()));
            }
            for option in opted.options {
                if option.kind == OptionType::Channel && !option.channel_types.is_empty() {
                    out.push(refuse(format!(
                        "its option `{}` declares `channel_types`",
                        option.name
                    )));
                }
            }
        }
        for (index, opted) in self.commands.iter().enumerate() {
            for other in &self.commands[..index] {
                for path in opted.paths.iter().filter(|p| other.paths.contains(p)) {
                    out.push(format!(
                        "Command Path `{path}` is opted in by both `{}` and `{}`: opt in only one",
                        other.name, opted.name
                    ));
                }
            }
        }
        out
    }
}

impl Plugin for PrefixPlugin {
    fn install(&self, app: &mut AppBuilder) {
        for message in self.violations() {
            app.report(BuildViolation::plugin::<Self>(message));
        }
        let table = Arc::new(Table::new(&self.prefix, &self.commands));
        let handler = move |ctx: EventCtx, event: MessageCreate| {
            let table = Arc::clone(&table);
            async move { on_message(&table, &ctx, event.0).await }
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
async fn on_message(table: &Table, ctx: &EventCtx, message: Message) -> HandlerResult<()> {
    if parse::is_ignored(&message) {
        return Ok(());
    }
    let Some((entry, args)) = table.find(&message.content) else {
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
            .shard(ctx.shard());
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
    use noctis_core::__rules::command::OptionDecl;
    use noctis_core::{CommandMetadata, InteractionContext, Permissions};

    use super::*;

    struct A;
    struct B;

    fn opted<T: 'static>(name: &'static str, paths: &'static [&'static str]) -> Opted {
        Opted {
            type_id: TypeId::of::<T>(),
            name,
            paths,
            options: &[],
            metadata: CommandMetadata::new(),
        }
    }

    fn plugin(prefix: &str) -> PrefixPlugin {
        PrefixPlugin::new(prefix).opt(opted::<A>("bot::A", &["warn"]))
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
    fn p4_no_commands() {
        assert_eq!(
            only(&PrefixPlugin::new("!")),
            "no Command is opted in: add `.command::<T>()`"
        );
    }

    /// P5: one violation per unenforceable piece of metadata.
    #[test]
    fn p5_unenforceable_metadata() {
        static CHANNEL: [OptionDecl<'static>; 1] =
            [OptionDecl::new("where", OptionType::Channel, "d").channel_types(&[0])];
        static PLAIN_CHANNEL: [OptionDecl<'static>; 1] =
            [OptionDecl::new("where", OptionType::Channel, "d")];
        let mut perms = opted::<A>("bot::Ban", &["ban"]);
        perms.metadata =
            CommandMetadata::new().default_member_permissions(Permissions::BAN_MEMBERS);
        let mut nsfw = opted::<B>("bot::Lewd", &["lewd"]);
        nsfw.metadata = CommandMetadata::new().nsfw(true);
        let mut channel = opted::<u8>("bot::Move", &["move"]);
        channel.options = &CHANNEL;
        let violations = PrefixPlugin::new("!")
            .opt(perms)
            .opt(nsfw)
            .opt(channel)
            .violations();
        assert_eq!(violations.len(), 3, "{violations:?}");
        assert!(violations[0].starts_with("`bot::Ban` cannot be a prefix command"));
        assert!(violations[0].contains("`permissions`"));
        assert!(violations[1].starts_with("`bot::Lewd`") && violations[1].contains("nsfw"));
        assert!(violations[2].starts_with("`bot::Move`"));
        assert!(violations[2].contains("`where` declares `channel_types`"));

        let mut fine = opted::<A>("bot::Fine", &["fine"]);
        fine.metadata = CommandMetadata::new()
            .nsfw(false)
            .contexts(&[InteractionContext::Guild]);
        fine.options = &PLAIN_CHANNEL;
        assert!(PrefixPlugin::new("!").opt(fine).violations().is_empty());
    }

    /// P6: a shared path between two types; one type twice is deduplicated.
    #[test]
    fn p6_path_clash() {
        let message = only(
            &PrefixPlugin::new("!")
                .opt(opted::<A>("bot::A", &["config set", "warn"]))
                .opt(opted::<B>("bot::B", &["warn"])),
        );
        assert!(message.starts_with("Command Path `warn`"), "{message}");
        assert!(message.contains("`bot::A`") && message.contains("`bot::B`"));
        let twice = PrefixPlugin::new("!")
            .opt(opted::<A>("bot::A", &["warn"]))
            .opt(opted::<A>("bot::A", &["warn"]));
        assert_eq!(twice.commands.len(), 1);
        assert!(twice.violations().is_empty());
    }
}
