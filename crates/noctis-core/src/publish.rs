//! Publish: writes the Registry's commands to Discord, only when they
//! differ from what Discord holds (FR-11, FR-13, AD-7, AD-26, META-04,
//! REG-05).
//!
//! One Publish reads the application (`GET /applications/@me`), builds the
//! command JSON of every target from the Registry alone, runs every check,
//! then per target reads the remote commands, diffs both canonical forms
//! and sends one bulk-overwrite `PUT` only when the diff is not empty (and
//! this is not a dry run). It runs in the `noctis.publish` span.
//!
//! ```no_run
//! use noctis_core::publish::PublishScope;
//! use twilight_model::id::Id;
//!
//! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let app = noctis_core::App::from_env()?.build()?;
//! let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
//! println!("{report}");
//! # Ok(()) }
//! ```

mod diff;
mod json;

use std::collections::HashSet;
use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;

use noctis_rules::Limits;
use noctis_rules::command::{IntegrationType, Scope};
use serde_json::Value;
use tracing::Instrument as _;
use twilight_http::error::ErrorType;
use twilight_http::request::Request;
use twilight_http::response::marker::EmptyBody;
use twilight_http::routing::Route;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::App;
use crate::edge::EDGE_BODY_PREFIX;
use crate::registry::commands::Commands;
use diff::{Defaults, canonical, diff};

/// Where a Publish writes.
///
/// ```
/// use noctis_core::publish::PublishScope;
/// use twilight_model::id::Id;
///
/// assert_eq!(PublishScope::Global.to_string(), "global");
/// assert_eq!(PublishScope::Guild(Id::new(42)).to_string(), "guild 42");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublishScope {
    /// The global commands (`--publish`), then each guild-scoped Command's
    /// guilds. With [`Publish::clear`], only the global commands.
    Global,
    /// One guild (`--publish-guild <id>`, the dev mirror): every global
    /// Command (without `contexts` and `integration_types`, META-03) and
    /// that guild's own Commands. With [`Publish::clear`], that guild's
    /// commands.
    Guild(Id<GuildMarker>),
}

impl fmt::Display for PublishScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => f.write_str("global"),
            Self::Guild(id) => write!(f, "guild {id}"),
        }
    }
}

/// Why a Publish failed. Every check runs before the first `PUT`, so a
/// failed check writes nothing; the first HTTP error stops the remaining
/// targets. No message contains the token or a request body.
///
/// ```
/// use noctis_core::publish::PublishError;
///
/// assert!(PublishError::MissingToken.to_string().contains("NOCTIS_TOKEN"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PublishError {
    /// The App has no token; nothing was sent.
    #[error("Publish needs a bot token: set NOCTIS_TOKEN or call `AppBuilder::token`")]
    MissingToken,
    /// Discord answered with an error status.
    #[error(
        "Discord rejected `{route}` with HTTP {status}{}: check the token, the application and \
         the commands",
        CodeSuffix(*code)
    )]
    Rejected {
        /// The method and path, e.g. `PUT /applications/7/commands`.
        route: String,
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// The request did not complete (connection, timeout, unreadable body).
    #[error(
        "`{route}` did not complete (network error or timeout): check the connection and retry"
    )]
    Transport {
        /// The method and path.
        route: String,
    },
    /// Discord's answer is not what the route returns.
    #[error("`{route}` returned an unexpected body ({reason}): retry, or report it")]
    InvalidResponse {
        /// The method and path.
        route: String,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A global command declares an installation type the application has
    /// not enabled (META-04).
    #[error(
        "[META-04] command `{command}` declares integration type `{}`, which this application \
         has not enabled: enable it under Installation in the Discord Developer Portal, or \
         remove it from `integration_types`",
        install_name(*integration_type)
    )]
    IntegrationTypeNotEnabled {
        /// The command name.
        command: String,
        /// The disabled installation type.
        integration_type: IntegrationType,
    },
    /// Two commands of one type share a name in one guild (REG-03): the dev
    /// mirror unites the global Commands with the guild's own.
    #[error(
        "[REG-03] two {kind} commands are named `{name}` in guild {guild} (a global Command and \
         a Command of that guild, mirrored by --publish-guild): rename one"
    )]
    NameClash {
        /// The guild.
        guild: Id<GuildMarker>,
        /// `chat_input`, `user` or `message`.
        kind: &'static str,
        /// The shared name.
        name: String,
    },
    /// One guild would hold too many commands of one type (REG-02).
    #[error(
        "[REG-02] guild {guild} would hold {count} {kind} commands (at most {max}; the \
         --publish-guild mirror counts the global Commands too): remove some, or publish \
         globally"
    )]
    TooManyCommands {
        /// The guild.
        guild: Id<GuildMarker>,
        /// `chat_input`, `user` or `message`.
        kind: &'static str,
        /// The number of commands.
        count: usize,
        /// The limit.
        max: usize,
    },
}

/// ` (code N)` or nothing.
struct CodeSuffix(Option<u32>);

impl fmt::Display for CodeSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(code) => write!(f, " (code {code})"),
            None => Ok(()),
        }
    }
}

/// The snake-case name Discord's docs use for an installation type.
fn install_name(kind: IntegrationType) -> &'static str {
    match kind {
        IntegrationType::GuildInstall => "guild_install",
        IntegrationType::UserInstall => "user_install",
    }
}

/// What a Publish did for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetReport {
    scope: PublishScope,
    json: String,
    diff: String,
    creates: usize,
    wrote: bool,
}

impl TargetReport {
    /// Where the target is: [`PublishScope::Global`] for the global
    /// commands, else its guild.
    pub fn scope(&self) -> PublishScope {
        self.scope
    }

    /// The local command JSON (pretty-printed), as built from the Registry.
    pub fn json(&self) -> &str {
        &self.json
    }

    /// One line per change, sorted (creates, updates, deletes; then by
    /// command type and name), e.g. ``+ create chat_input `ping` ``,
    /// ``~ update chat_input `cfg` (description)``,
    /// ``- delete user `Report` ``. Empty when local equals remote.
    pub fn diff(&self) -> &str {
        &self.diff
    }

    /// How many commands a write creates (the `+` lines; REG-05 allows 200
    /// creates per day per guild).
    pub fn creates(&self) -> usize {
        self.creates
    }

    /// Whether the local commands differ from the remote ones.
    pub fn changed(&self) -> bool {
        !self.diff.is_empty()
    }

    /// Whether the `PUT` was sent.
    pub fn wrote(&self) -> bool {
        self.wrote
    }
}

/// What a Publish did. `Display` is the printout of the CLI modes: per
/// target the JSON, the diff and what happened, then the create count.
///
/// ```no_run
/// use noctis_core::publish::PublishScope;
///
/// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let app = noctis_core::App::from_env()?.build()?;
/// let report = app.publish(PublishScope::Global).dry_run(true).await?;
/// for target in report.targets() {
///     println!("{}: {} creates\n{}", target.scope(), target.creates(), target.diff());
/// }
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReport {
    targets: Vec<TargetReport>,
    dry_run: bool,
}

impl PublishReport {
    /// One report per target, in publish order.
    pub fn targets(&self) -> &[TargetReport] {
        &self.targets
    }

    /// The creates over every target (REG-05).
    pub fn creates(&self) -> usize {
        self.targets.iter().map(TargetReport::creates).sum()
    }

    /// Whether any `PUT` was sent.
    pub fn wrote(&self) -> bool {
        self.targets.iter().any(TargetReport::wrote)
    }

    /// Whether this was a dry run (nothing written).
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}

impl fmt::Display for PublishReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for target in &self.targets {
            writeln!(f, "== {} ==", target.scope)?;
            writeln!(f, "{}", target.json)?;
            if target.diff.is_empty() {
                writeln!(f, "no changes")?;
            } else {
                writeln!(f, "{}", target.diff)?;
            }
            let outcome = if target.wrote {
                "written"
            } else if !target.changed() {
                "unchanged; nothing written"
            } else {
                "dry run; nothing written"
            };
            writeln!(f, "{outcome}")?;
        }
        write!(
            f,
            "creates: {} (REG-05: Discord allows 200 command creates per day per guild)",
            self.creates()
        )
    }
}

/// A pending Publish, from [`App::publish`]; `.await` it.
///
/// ```no_run
/// use noctis_core::publish::PublishScope;
///
/// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let app = noctis_core::App::from_env()?.build()?;
/// let report = app.publish(PublishScope::Global).await?;
/// assert!(!report.is_dry_run());
/// # Ok(()) }
/// ```
#[must_use = "a Publish does nothing until it is awaited"]
#[derive(Debug)]
pub struct Publish<'a> {
    app: &'a App,
    scope: PublishScope,
    dry_run: bool,
    clear: bool,
}

impl<'a> Publish<'a> {
    pub(crate) fn new(app: &'a App, scope: PublishScope) -> Self {
        Self {
            app,
            scope,
            dry_run: false,
            clear: false,
        }
    }

    /// Reads and diffs, runs every check, but writes nothing.
    pub fn dry_run(self, dry_run: bool) -> Self {
        Self { dry_run, ..self }
    }

    /// Publishes an empty command list to the scope's one target instead
    /// (`--publish-clear`): deletes the App's commands there.
    pub fn clear(self, clear: bool) -> Self {
        Self { clear, ..self }
    }
}

impl<'a> IntoFuture for Publish<'a> {
    type Output = Result<PublishReport, PublishError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        let span = tracing::info_span!(
            "noctis.publish",
            app = %self.app.app_label(),
            scope = %self.scope,
            dry_run = self.dry_run,
            clear = self.clear,
        );
        Box::pin(execute(self).instrument(span))
    }
}

/// The application fields Publish needs.
struct Application {
    id: u64,
    /// The enabled installation types (`integration_types_config` keys),
    /// sorted.
    enabled: Vec<u64>,
}

/// One `PUT` target.
struct Target {
    scope: PublishScope,
    local: Vec<Value>,
    /// Whether META-04 applies (the global target of a non-clear Publish).
    meta_04: bool,
}

async fn execute(publish: Publish<'_>) -> Result<PublishReport, PublishError> {
    let Publish {
        app,
        scope,
        dry_run,
        clear,
    } = publish;
    if !app.has_token() {
        return Err(PublishError::MissingToken);
    }
    let http = app.http();
    let route = Route::GetCurrentUserApplicationInfo;
    let label = "GET /applications/@me".to_owned();
    let body = send(http, &route, &label, None).await?;
    let application = parse_application(&body).map_err(|reason| PublishError::InvalidResponse {
        route: label,
        reason,
    })?;
    let targets = targets(app.registry().commands(), scope, clear)?;
    for target in targets.iter().filter(|t| t.meta_04) {
        check_meta_04(&target.local, &application.enabled)?;
    }
    let mut reports = Vec::with_capacity(targets.len());
    for target in targets {
        reports.push(write_target(http, &application, target, dry_run).await?);
    }
    Ok(PublishReport {
        targets: reports,
        dry_run,
    })
}

/// Every target of one Publish, with the dev-mirror checks (REG-03,
/// REG-02) already run.
fn targets(
    commands: &Commands,
    scope: PublishScope,
    clear: bool,
) -> Result<Vec<Target>, PublishError> {
    let local_of = |scope: Scope| commands.scope(scope).map(json::local).unwrap_or_default();
    if clear {
        return Ok(vec![Target {
            scope,
            local: Vec::new(),
            meta_04: false,
        }]);
    }
    match scope {
        PublishScope::Global => {
            let mut targets = vec![Target {
                scope,
                local: local_of(Scope::Global),
                meta_04: true,
            }];
            for scoped in commands.scopes() {
                if let Scope::Guild(id) = scoped.scope
                    && let Some(guild) = Id::new_checked(id)
                {
                    targets.push(Target {
                        scope: PublishScope::Guild(guild),
                        local: json::local(scoped),
                        meta_04: false,
                    });
                }
            }
            Ok(targets)
        }
        PublishScope::Guild(guild) => {
            let mut local = local_of(Scope::Global);
            for command in &mut local {
                json::strip_install(command);
            }
            local.extend(local_of(Scope::Guild(guild.get())));
            check_mirror(guild, &local)?;
            Ok(vec![Target {
                scope,
                local,
                meta_04: false,
            }])
        }
    }
}

/// The command `type` of a JSON command (absent means 1).
fn command_type(command: &Value) -> Option<u64> {
    match command.get("type") {
        None | Some(Value::Null) => Some(1),
        Some(value) => value.as_u64(),
    }
}

/// The diff and error name of a command type.
fn kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "chat_input",
        2 => "user",
        3 => "message",
        _ => "other",
    }
}

/// REG-03 then REG-02 on the dev mirror's union.
fn check_mirror(guild: Id<GuildMarker>, local: &[Value]) -> Result<(), PublishError> {
    let mut seen: HashSet<(u64, &str)> = HashSet::new();
    for command in local {
        let kind = command_type(command).unwrap_or(1);
        let name = command.get("name").and_then(Value::as_str).unwrap_or("");
        if !seen.insert((kind, name)) {
            return Err(PublishError::NameClash {
                guild,
                kind: kind_name(kind),
                name: name.to_owned(),
            });
        }
    }
    let limits = Limits::default();
    for (kind, max) in [
        (1, limits.chat_input_commands),
        (2, limits.user_commands),
        (3, limits.message_commands),
    ] {
        let count = local
            .iter()
            .filter(|c| command_type(c) == Some(kind))
            .count();
        if count > max {
            return Err(PublishError::TooManyCommands {
                guild,
                kind: kind_name(kind),
                count,
                max,
            });
        }
    }
    Ok(())
}

/// META-04: every declared `integration_types` value must be enabled.
fn check_meta_04(local: &[Value], enabled: &[u64]) -> Result<(), PublishError> {
    for command in local {
        let Some(types) = command.get("integration_types").and_then(Value::as_array) else {
            continue;
        };
        for value in types.iter().filter_map(Value::as_u64) {
            if enabled.contains(&value) {
                continue;
            }
            if let Some(&integration_type) = IntegrationType::ALL
                .iter()
                .find(|t| u64::from(t.value()) == value)
            {
                return Err(PublishError::IntegrationTypeNotEnabled {
                    command: command
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    integration_type,
                });
            }
        }
    }
    Ok(())
}

/// Reads, diffs and (when changed and not a dry run) writes one target.
async fn write_target(
    http: &twilight_http::Client,
    application: &Application,
    target: Target,
    dry_run: bool,
) -> Result<TargetReport, PublishError> {
    let Target { scope, local, .. } = target;
    let application_id = application.id;
    let (get, put, path) = match scope {
        PublishScope::Global => (
            Route::GetGlobalCommands {
                application_id,
                with_localizations: Some(true),
            },
            Route::SetGlobalCommands { application_id },
            format!("/applications/{application_id}/commands"),
        ),
        PublishScope::Guild(guild) => (
            Route::GetGuildCommands {
                application_id,
                guild_id: guild.get(),
                with_localizations: Some(true),
            },
            Route::SetGuildCommands {
                application_id,
                guild_id: guild.get(),
            },
            format!("/applications/{application_id}/guilds/{guild}/commands"),
        ),
    };
    let get_label = format!("GET {path}");
    let body = send(http, &get, &get_label, None).await?;
    let defaults = Defaults {
        integration_types: application.enabled.clone(),
        guild: matches!(scope, PublishScope::Guild(_)),
    };
    let RemoteCommands {
        canonical: remote,
        preserved,
    } = split_remote(&body, &defaults).map_err(|reason| PublishError::InvalidResponse {
        route: get_label,
        reason,
    })?;
    let local_canonical: Vec<_> = local
        .iter()
        .filter_map(|command| canonical(command, &defaults))
        .collect();
    let diff = diff(&local_canonical, &remote);
    let mut local = Value::Array(local);
    let json = serde_json::to_string_pretty(&local).unwrap_or_default();
    let text = diff.lines.join("\n");
    let changed = !diff.lines.is_empty();
    let creates = diff.creates;
    let mut wrote = false;
    if !changed {
        tracing::info!(target: "noctis::publish", scope = %scope, "commands unchanged; nothing written");
    } else if dry_run {
        tracing::info!(
            target: "noctis::publish",
            scope = %scope,
            creates,
            diff = %text,
            "dry run: commands differ; nothing written"
        );
    } else {
        tracing::info!(
            target: "noctis::publish",
            scope = %scope,
            creates,
            diff = %text,
            "writing commands"
        );
        if let Value::Array(items) = &mut local {
            items.extend(preserved);
        }
        let body = serde_json::to_vec(&local).map_err(|_| PublishError::Transport {
            route: format!("PUT {path}"),
        })?;
        send(http, &put, &format!("PUT {path}"), Some(body)).await?;
        wrote = true;
    }
    Ok(TargetReport {
        scope,
        json,
        diff: text,
        creates,
        wrote,
    })
}

/// A parsed remote command list.
struct RemoteCommands {
    /// The chat-input, user and message commands, canonical.
    canonical: Vec<(diff::Key, Value)>,
    /// Every other command (e.g. an Entry Point), verbatim.
    preserved: Vec<Value>,
}

/// A remote command list: the canonical chat-input, user and message
/// commands, and every other command verbatim (kept by a `PUT`).
fn split_remote(body: &[u8], defaults: &Defaults) -> Result<RemoteCommands, &'static str> {
    let remote: Value = serde_json::from_slice(body).map_err(|_| "not JSON")?;
    let Value::Array(remote) = remote else {
        return Err("not an array of commands");
    };
    let mut canonical_commands = Vec::with_capacity(remote.len());
    let mut preserved = Vec::new();
    for command in remote {
        if !command.is_object() {
            return Err("a command is not an object");
        }
        match command_type(&command) {
            Some(1..=3) => {
                canonical_commands
                    .push(canonical(&command, defaults).ok_or("a command has no name")?);
            }
            _ => preserved.push(command),
        }
    }
    Ok(RemoteCommands {
        canonical: canonical_commands,
        preserved,
    })
}

/// The application `id` and enabled installation types of a
/// `GET /applications/@me` body. A missing or null
/// `integration_types_config` means only guild install (0).
fn parse_application(body: &[u8]) -> Result<Application, &'static str> {
    let value: Value = serde_json::from_slice(body).map_err(|_| "not JSON")?;
    let id = match value.get("id") {
        Some(Value::String(id)) => id.parse::<u64>().ok(),
        Some(Value::Number(id)) => id.as_u64(),
        _ => None,
    }
    .filter(|&id| id != 0)
    .ok_or("no application id")?;
    let mut enabled: Vec<u64> = match value.get("integration_types_config") {
        Some(Value::Object(config)) => config.keys().filter_map(|k| k.parse().ok()).collect(),
        _ => vec![0],
    };
    enabled.sort_unstable();
    enabled.dedup();
    Ok(Application { id, enabled })
}

/// Sends one request with the bot token through the App's twilight client;
/// the response body.
async fn send(
    http: &twilight_http::Client,
    route: &Route<'_>,
    label: &str,
    body: Option<Vec<u8>>,
) -> Result<Vec<u8>, PublishError> {
    let transport = || PublishError::Transport {
        route: label.to_owned(),
    };
    let mut builder = Request::builder(route);
    if let Some(body) = body {
        builder = builder.body(body);
    }
    let request = builder.build().map_err(|error| {
        tracing::debug!(target: "noctis::publish", %error, "request not built");
        transport()
    })?;
    let response = http
        .request::<EmptyBody>(request)
        .await
        .map_err(|error| http_error(&error, label))?;
    response.bytes().await.map_err(|error| {
        tracing::debug!(target: "noctis::publish", %error, "response body not read");
        transport()
    })
}

/// Maps a twilight error: an error status to [`PublishError::Rejected`]
/// (the edge's own 502, upstream unreachable, and anything else to
/// [`PublishError::Transport`]).
fn http_error(error: &twilight_http::Error, label: &str) -> PublishError {
    match error.kind() {
        ErrorType::Response { body, status, .. }
            if !(status.get() == 502 && body.starts_with(EDGE_BODY_PREFIX.as_bytes())) =>
        {
            PublishError::Rejected {
                route: label.to_owned(),
                status: status.get(),
                code: error_code(body),
            }
        }
        ErrorType::Unauthorized => PublishError::Rejected {
            route: label.to_owned(),
            status: 401,
            code: None,
        },
        _ => {
            tracing::debug!(target: "noctis::publish", %error, "request failed");
            PublishError::Transport {
                route: label.to_owned(),
            }
        }
    }
}

/// Discord's JSON error `code`, if the body has one.
fn error_code(body: &[u8]) -> Option<u32> {
    let value: Value = serde_json::from_slice(body).ok()?;
    u32::try_from(value.get("code")?.as_u64()?).ok()
}

#[cfg(test)]
mod tests;
