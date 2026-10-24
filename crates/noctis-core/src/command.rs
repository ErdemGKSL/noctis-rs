//! Slash Commands: the [`CommandHandler`] descriptor `#[command]` expands to,
//! the [`CommandCtx`] handlers receive, the [`CommandPath`] trait of the
//! generated path enums and [`UnknownCommandPath`] (FR-7, FR-8, AD-8, AD-26).

use std::fmt;
use std::future::Future;
use std::sync::{Arc, LazyLock, OnceLock};

use futures_util::future::BoxFuture;
use noctis_rules::command::OptionDecl;
use noctis_rules::{Limits, Locale};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::autocomplete::{__AutocompleteMethod, erased_for};
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::locale::{LocaleCell, Locales};
use crate::metadata::CommandMetadata;
use crate::options::{CommandInput, Options};
use crate::rate_limits::{__RateLimitedHook, RateLimit, RateLimitedFn, RateLimits, RateRun};
use crate::responder::{
    Command, CommandResponder, DeferMode, Ephemerality, IntoResponse, Responder, Response,
    ResponseValue, Shared, Transport,
};

/// The typed Command Paths of one Command Pattern; implemented by
/// `#[command]` for the `<Fn>Path` enum it generates.
///
/// ```
/// use noctis_core::CommandPath;
///
/// #[derive(Clone, Copy, Debug, PartialEq)]
/// enum ModPath { Ban, Kick }
///
/// impl CommandPath for ModPath {
///     const PATHS: &'static [&'static str] = &["mod ban", "mod kick"];
///     fn from_path(path: &str) -> Option<Self> {
///         match path {
///             "mod ban" => Some(Self::Ban),
///             "mod kick" => Some(Self::Kick),
///             _ => None,
///         }
///     }
///     fn as_str(self) -> &'static str {
///         match self {
///             Self::Ban => "mod ban",
///             Self::Kick => "mod kick",
///         }
///     }
/// }
///
/// assert_eq!(ModPath::from_path("mod kick"), Some(ModPath::Kick));
/// assert_eq!(ModPath::Ban.as_str(), ModPath::PATHS[0]);
/// ```
pub trait CommandPath: Copy + Send + Sync + 'static {
    /// Every path, in expansion order (words joined by one ASCII space).
    const PATHS: &'static [&'static str];

    /// The variant of `path` (an exact byte match), or `None`.
    fn from_path(path: &str) -> Option<Self>;

    /// The variant's Command Path.
    fn as_str(self) -> &'static str;
}

/// The context a Command handler receives: the invoked path, the Module,
/// the shard, the guild, channel and user, the locales, the declared
/// ephemerality, the App's typed state and its HTTP client.
///
/// Cheap to clone (one `Arc`). `Debug` leaves the HTTP client out.
///
/// ```
/// use noctis_core::{CommandCtx, HandlerResult};
/// use noctis_ui::{Components, TextDisplay};
///
/// async fn hello(ctx: CommandCtx) -> HandlerResult<Components> {
///     let text = format!("hello <@{}> ({})", ctx.user_id(), ctx.response_locale());
///     Ok(Components::new().push(TextDisplay::new(text)))
/// }
/// ```
///
/// Answer with [`respond`](Self::respond) or
/// [`reply_text`](Self::reply_text). A Command has no message to update,
/// so `CommandCtx` has no `update`:
///
/// ```compile_fail,E0599
/// use noctis_core::{CommandCtx, HandlerResult, Responded};
/// use noctis_ui::{Components, TextDisplay, V2Message};
///
/// async fn update(ctx: CommandCtx) -> HandlerResult<Responded> {
///     let m = V2Message::new(Components::new().push(TextDisplay::new("hi")));
///     ctx.update(m).await // [INT-04]
/// }
/// ```
#[derive(Clone)]
pub struct CommandCtx(Arc<CtxInner>);

crate::responder::ergonomics::ctx_methods!(CommandCtx);

/// The parts of a [`CommandCtx`].
pub(crate) struct CtxInner {
    pub(crate) path: &'static str,
    pub(crate) input: CommandInput,
    pub(crate) module: &'static str,
    pub(crate) shard: u32,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    pub(crate) user_id: Id<UserMarker>,
    /// The run's resolved locales (stage 4, then the Middleware
    /// overrides); shared with the Responder, the notices and the run.
    pub(crate) locales: Arc<LocaleCell>,
    pub(crate) ephemerality: Ephemerality,
    /// `None` for the detached `__testing` seams.
    pub(crate) app: Option<Arc<AppShared>>,
    /// The Ref the clicked or submitted `custom_id` carried; `None` for an
    /// inline id and for every Command (a Command has no Ref).
    pub(crate) current_ref: Option<noctis_codec::Ref>,
    /// The run whose frozen Extensions the handler's `Ext<T>` parameters
    /// read; `None` without Middleware (and in the detached seams).
    pub(crate) run: Option<Arc<crate::middleware::Run>>,
    /// The member's resolved channel permissions (`member.permissions`);
    /// `None` in DMs and for every Command.
    pub(crate) member_permissions: Option<crate::Permissions>,
    /// The run's responder state, which `ctx.respond`, `ctx.update` and
    /// `ctx.reply_text` answer through; `None` only for contexts built by
    /// hand in crate unit tests (they answer `NoReply`).
    pub(crate) shared: Option<Arc<Shared>>,
    /// The values a select click picked; empty for every other
    /// interaction.
    pub(crate) values: crate::component::SelectedValues,
    /// The components Noctis does not model: of the clicked message
    /// (`message.components`) for a click, of the submit for a modal
    /// submit; empty otherwise (FR-31, CV2-16).
    pub(crate) unknown: Vec<serde_json::Value>,
    /// The run's declared rate limits and their keys (stage 5,
    /// `ctx.rate_limits()`); `None` in the detached seams.
    pub(crate) rate: Option<Arc<RateRun>>,
}

impl CtxInner {
    /// The run's frozen Extensions, or the empty map.
    pub(crate) fn extensions(&self) -> &crate::Extensions {
        match &self.run {
            Some(run) => run.extensions(),
            None => crate::Extensions::empty(),
        }
    }

    /// The run's current locales.
    pub(crate) fn locales(&self) -> Locales {
        self.locales.get()
    }

    /// The response locale (FR-43): the User Locale for an ephemeral
    /// response, the Guild Locale for a public one.
    pub(crate) fn response_locale(&self) -> Locale {
        let locales = self.locales();
        match self.ephemerality {
            Ephemerality::Ephemeral => locales.user,
            Ephemerality::Public => locales.guild,
        }
    }
}

impl CtxInner {
    /// Detached parts: no path, App, guild or locale; shard 0, user 1.
    pub(crate) fn detached_parts(ephemerality: Ephemerality) -> Self {
        Self {
            path: "",
            input: CommandInput::empty(""),
            module: "",
            shard: 0,
            guild_id: None,
            channel_id: None,
            user_id: DETACHED_USER,
            locales: LocaleCell::new(Locales::both(Locale::EnUs)),
            ephemerality,
            app: None,
            current_ref: None,
            run: None,
            member_permissions: None,
            shared: None,
            values: crate::component::SelectedValues::default(),
            unknown: Vec::new(),
            rate: None,
        }
    }

    /// A copy of the parts a [`RateLimitedCtx`](crate::RateLimitedCtx)
    /// reads (the input, the responder state and the run are left out).
    pub(crate) fn rate_limited_parts(&self) -> Self {
        Self {
            path: self.path,
            module: self.module,
            shard: self.shard,
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            user_id: self.user_id,
            locales: Arc::clone(&self.locales),
            app: self.app.clone(),
            member_permissions: self.member_permissions,
            rate: self.rate.clone(),
            ..Self::detached_parts(self.ephemerality)
        }
    }

    /// The run's rate limits handle.
    pub(crate) fn rate_limits(&self) -> RateLimits {
        RateLimits::of(self.rate.as_ref())
    }
}

/// The placeholder user of a detached context.
const DETACHED_USER: Id<UserMarker> = Id::new(1);

impl CommandCtx {
    /// A context of `parts`.
    pub(crate) fn from_parts(parts: CtxInner) -> Self {
        Self(Arc::new(parts))
    }

    /// A detached context (the `__testing` seams) bound to `shared`: no
    /// App, shard 0, user 1.
    pub(crate) fn detached(
        path: &'static str,
        input: CommandInput,
        ephemerality: Ephemerality,
        shared: &Arc<Shared>,
    ) -> Self {
        Self::from_parts(CtxInner {
            path,
            input,
            shared: Some(Arc::clone(shared)),
            ..CtxInner::detached_parts(ephemerality)
        })
    }

    /// An unbound context for an invocation of `path` without options.
    #[cfg(test)]
    pub(crate) fn new(path: &'static str) -> Self {
        Self::from_parts(CtxInner {
            path,
            input: CommandInput::empty(path),
            ..CtxInner::detached_parts(Ephemerality::Public)
        })
    }

    /// The decoded interaction input.
    pub(crate) fn input(&self) -> &CommandInput {
        &self.0.input
    }

    /// The invoked Command Path, words joined by one ASCII space
    /// (`"config set prefix"`); for a context-menu command, its menu name
    /// (`"Show profile"`).
    pub fn command_path(&self) -> &'static str {
        self.0.path
    }

    /// The id of the Module the handler was added to.
    pub fn module(&self) -> &'static str {
        self.0.module
    }

    /// The number of the shard that received the interaction.
    pub fn shard(&self) -> u32 {
        self.0.shard
    }

    /// The guild the Command was used in, if any.
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.0.guild_id
    }

    /// The channel the Command was used in, if Discord sent it.
    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.0.channel_id
    }

    /// The invoking user.
    pub fn user_id(&self) -> Id<UserMarker> {
        self.0.user_id
    }

    /// The resolved User Locale (FR-43): Discord's `locale`, then the
    /// Guild Locale, then the App's default, each step taking the code's
    /// own locale file or its sibling's (`en-GB` → `en-US`); then the App's
    /// `LocaleResolver` and any before-Middleware override. Use it for
    /// ephemeral, single-user UI.
    pub fn user_locale(&self) -> Locale {
        self.0.locales().user
    }

    /// The resolved Guild Locale (FR-43): Discord's `guild_locale`, then
    /// the User Locale (in a DM, or when the guild's code has no file),
    /// then the App's default, resolved like
    /// [`user_locale`](Self::user_locale) and overridable the same way. Use
    /// it for shared content.
    pub fn guild_locale(&self) -> Locale {
        self.0.locales().guild
    }

    /// The locale the response uses (FR-43): the User Locale for a Command
    /// declared `response = ephemeral`, the Guild Locale for a public one.
    /// Pass it to `Strings::of`.
    pub fn response_locale(&self) -> Locale {
        self.0.response_locale()
    }

    /// The ephemerality the Command declared
    /// (`#[command(.., response = ephemeral)]`; `response = public` is the
    /// default).
    pub fn ephemerality(&self) -> Ephemerality {
        self.0.ephemerality
    }

    /// The state registered for `T` with `AppBuilder::state`, if any.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.0.app.as_ref()?.states.get::<T>()
    }

    /// The handler's declared rate limits (`rate_limit(..)` on its
    /// `#[command]` or context-menu attribute): count, block, extend or
    /// reset them for the next invocation (FR-46). Every call of a detached
    /// test context is `RateLimitError::Detached`.
    pub fn rate_limits(&self) -> RateLimits {
        self.0.rate_limits()
    }

    /// The App's typed key-value handle over its namespaced `CacheStore`
    /// (a cheap clone).
    pub fn kv(&self) -> crate::Kv {
        crate::Kv::of(self.0.app.as_ref())
    }

    /// The App's Discord resource cache (a cheap clone): owned values,
    /// `Ok(None)` when not cached, [`CacheError::Disabled`](crate::CacheError::Disabled)
    /// when the resource is not configured or without an App.
    pub fn cache(&self) -> crate::DiscordCacheReader {
        crate::DiscordCacheReader::of(self.0.app.as_ref())
    }

    /// The App's webhook factory: create a webhook the App owns, or wrap
    /// stored credentials (a cheap clone); every request of a detached
    /// test seam's factory is `WebhookError::Detached`.
    pub fn webhooks(&self) -> crate::Webhooks {
        crate::Webhooks::of(self.0.app.as_ref())
    }

    /// The App's one Discord HTTP client (the `Arc` `App::http` returns);
    /// an unconfigured client in the detached test seams.
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        match &self.0.app {
            Some(app) => &app.http,
            None => detached_http(),
        }
    }

    /// The App's gateway command handle (the one `App::gateway` returns):
    /// set the presence, request guild members from the owning shard; a
    /// detached handle (every call `GatewayError::NotRunning`) in the
    /// detached test seams.
    pub fn gateway(&self) -> noctis_gateway::GatewayHandle {
        self.0
            .app
            .as_ref()
            .map_or_else(noctis_gateway::GatewayHandle::detached, |app| {
                app.gateway.clone()
            })
    }

    /// The App's Limits (`AppBuilder::limits`), what the Responder
    /// validates against; pass it to render functions that size pages with
    /// `noctis::ui::Budget`. [`Limits::default`] in the detached test
    /// seams.
    pub fn limits(&self) -> &Limits {
        limits_of(self.0.app.as_deref())
    }
}

/// The Limits of contexts without an App.
pub(crate) static DEFAULT_LIMITS: LazyLock<Limits> = LazyLock::new(Limits::default);

/// The App's Limits, or [`DEFAULT_LIMITS`] without an App.
pub(crate) fn limits_of(app: Option<&AppShared>) -> &Limits {
    app.map_or(&*DEFAULT_LIMITS, |app| &app.limits)
}

/// The client of detached contexts: no token, never used by noctis.
pub(crate) fn detached_http() -> &'static Arc<twilight_http::Client> {
    static HTTP: OnceLock<Arc<twilight_http::Client>> = OnceLock::new();
    HTTP.get_or_init(|| {
        // Building the client needs a rustls crypto provider, and its
        // builder spawns a rate limiter (dropped here) on a tokio runtime.
        let _ = crate::startup::ensure_crypto_provider();
        let runtime = tokio::runtime::Handle::try_current()
            .is_err()
            .then(|| tokio::runtime::Builder::new_current_thread().build().ok())
            .flatten();
        let _entered = runtime.as_ref().map(tokio::runtime::Runtime::enter);
        Arc::new(twilight_http::Client::builder().ratelimiter(None).build())
    })
}

impl crate::kind::__Extensions for CommandCtx {
    fn __extensions(&self) -> &crate::Extensions {
        self.0.extensions()
    }
}

impl fmt::Debug for CommandCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandCtx")
            .field("command_path", &self.0.path)
            .field("module", &self.0.module)
            .field("shard", &self.0.shard)
            .field("guild_id", &self.0.guild_id)
            .field("channel_id", &self.0.channel_id)
            .field("user_id", &self.0.user_id)
            .field("user_locale", &self.0.locales().user)
            .field("guild_locale", &self.0.locales().guild)
            .field("ephemerality", &self.0.ephemerality)
            .finish_non_exhaustive()
    }
}

/// A Command invoked with a path its handler does not declare.
///
/// The handler does not run; this is the error the descriptor returns
/// instead.
#[derive(Debug, thiserror::Error)]
#[error("`{path}` is not a path of the Command `{command}`")]
pub struct UnknownCommandPath {
    path: Box<str>,
    command: &'static str,
}

impl UnknownCommandPath {
    /// The path that was invoked.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The Command's handler path (`module_path!()::name`).
    pub fn command(&self) -> &'static str {
        self.command
    }
}

/// A started handler: what it returned, as the pipeline sends it.
pub(crate) type HandlerFuture = BoxFuture<'static, HandlerResult<ResponseValue>>;

/// The erased handler call: the handler builds its own responder (of its
/// defer mode) over the shared state.
type ErasedCommand = dyn Fn(CommandCtx, Arc<Shared>) -> HandlerFuture + Send + Sync;

/// Boxes a handler future, converting its value into a response.
pub(crate) fn boxed<R, Fut>(fut: Fut) -> HandlerFuture
where
    R: IntoResponse<Command>,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
{
    Box::pin(async move { fut.await.map(<R as IntoResponse<Command>>::__into_response) })
}

/// One slash Command handler, as `#[command]` builds it; add it to a
/// [`Module`](crate::Module).
///
/// Opaque and cheap to clone. `Debug` prints the name, the pattern, the
/// paths, the option names, the ephemerality and the metadata. Annotating
/// `async fn config` with `#[command(..)]` gives a `fn config()` that
/// returns its `CommandHandler`.
#[derive(Clone)]
pub struct CommandHandler {
    name: &'static str,
    pattern: &'static str,
    /// One description per path, parallel to `paths` (one entry when
    /// `paths` is empty).
    descriptions: Arc<[&'static str]>,
    paths: &'static [&'static str],
    options: &'static [OptionDecl<'static>],
    metadata: CommandMetadata,
    ephemerality: Ephemerality,
    auto_defer: bool,
    call: Arc<ErasedCommand>,
    /// The autocomplete handler linked to an option, by option name.
    autocomplete: fn(&str) -> Option<__AutocompleteMethod>,
    /// The Extension types its `Ext<T>` parameters require.
    requires: &'static [crate::kind::__ExtType],
    /// The declared `rate_limit(..)`s (INT-14).
    rate_limits: &'static [RateLimit],
    /// The struct's `#[on_rate_limited]` method.
    on_rate_limited: Option<Arc<RateLimitedFn>>,
}

impl CommandHandler {
    fn from_call<D: DeferMode>(
        name: &'static str,
        pattern: &'static str,
        description: &'static str,
        paths: &'static [&'static str],
        options: &'static [OptionDecl<'static>],
        call: Arc<ErasedCommand>,
    ) -> Self {
        let descriptions: Arc<[&'static str]> = if paths.is_empty() {
            Arc::from([description])
        } else {
            paths.iter().map(|_| description).collect()
        };
        Self {
            name,
            pattern,
            descriptions,
            paths,
            options,
            metadata: CommandMetadata::new(),
            ephemerality: Ephemerality::Public,
            auto_defer: D::AUTO_DEFER,
            call,
            autocomplete: |_| None,
            requires: &[],
            rate_limits: &[],
            on_rate_limited: None,
        }
    }

    /// Links the autocomplete handlers of the options struct `O`.
    fn with_autocomplete<O: Options>(self) -> Self {
        Self {
            autocomplete: erased_for::<O>,
            ..self
        }
    }

    /// A handler of the Command Pattern `pattern` whose paths are `P`'s;
    /// what `#[command]` expands to for a pattern with a group.
    #[doc(hidden)]
    pub fn __new<P, D, F, Fut, R>(
        name: &'static str,
        pattern: &'static str,
        description: &'static str,
        f: F,
    ) -> Self
    where
        P: CommandPath,
        D: DeferMode,
        F: Fn(CommandCtx, P, CommandResponder<D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, shared: Arc<Shared>| -> HandlerFuture {
            match P::from_path(ctx.command_path()) {
                Some(path) => boxed::<_, _>(f(ctx, path, Responder::new(shared))),
                None => unknown(ctx.command_path(), name),
            }
        };
        Self::from_call::<D>(name, pattern, description, P::PATHS, &[], Arc::new(call))
    }

    /// Like [`__new`](Self::__new) for a handler that also takes the options
    /// struct `O`: the path is checked, `O` is decoded and validated, then
    /// the handler runs.
    #[doc(hidden)]
    pub fn __new_with_options<P, O, D, F, Fut, R>(
        name: &'static str,
        pattern: &'static str,
        description: &'static str,
        f: F,
    ) -> Self
    where
        P: CommandPath,
        O: Options,
        D: DeferMode,
        F: Fn(CommandCtx, P, CommandResponder<D>, O) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, shared: Arc<Shared>| -> HandlerFuture {
            let Some(path) = P::from_path(ctx.command_path()) else {
                return unknown(ctx.command_path(), name);
            };
            match decode::<O>(&ctx) {
                Ok(options) => boxed::<_, _>(f(ctx, path, Responder::new(shared), options)),
                Err(error) => failed(error),
            }
        };
        Self::from_call::<D>(
            name,
            pattern,
            description,
            P::PATHS,
            O::OPTIONS,
            Arc::new(call),
        )
        .with_autocomplete::<O>()
    }

    /// A handler of the single Command Path `paths[0]`; what `#[command]`
    /// expands to for a pattern without a group.
    #[doc(hidden)]
    pub fn __new_plain<D, F, Fut, R>(
        name: &'static str,
        paths: &'static [&'static str],
        description: &'static str,
        f: F,
    ) -> Self
    where
        D: DeferMode,
        F: Fn(CommandCtx, CommandResponder<D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, shared: Arc<Shared>| -> HandlerFuture {
            if paths.contains(&ctx.command_path()) {
                boxed::<_, _>(f(ctx, Responder::new(shared)))
            } else {
                unknown(ctx.command_path(), name)
            }
        };
        let pattern = paths.first().copied().unwrap_or_default();
        Self::from_call::<D>(name, pattern, description, paths, &[], Arc::new(call))
    }

    /// Like [`__new_plain`](Self::__new_plain) for a handler that also takes
    /// the options struct `O`.
    #[doc(hidden)]
    pub fn __new_plain_with_options<O, D, F, Fut, R>(
        name: &'static str,
        paths: &'static [&'static str],
        description: &'static str,
        f: F,
    ) -> Self
    where
        O: Options,
        D: DeferMode,
        F: Fn(CommandCtx, CommandResponder<D>, O) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        let call = move |ctx: CommandCtx, shared: Arc<Shared>| -> HandlerFuture {
            if !paths.contains(&ctx.command_path()) {
                return unknown(ctx.command_path(), name);
            }
            match decode::<O>(&ctx) {
                Ok(options) => boxed::<_, _>(f(ctx, Responder::new(shared), options)),
                Err(error) => failed(error),
            }
        };
        let pattern = paths.first().copied().unwrap_or_default();
        Self::from_call::<D>(
            name,
            pattern,
            description,
            paths,
            O::OPTIONS,
            Arc::new(call),
        )
        .with_autocomplete::<O>()
    }

    /// The handler's path (`module_path!()::name`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The Command Path or Pattern as written in `#[command("…")]`.
    pub fn pattern(&self) -> &'static str {
        self.pattern
    }

    /// The description of the first path: the literal `description = "…"`
    /// or, with the `i18n` feature, `cmd.<path>.description` from the
    /// default-locale file (empty when there is neither, which does not
    /// compile).
    pub fn description(&self) -> &'static str {
        self.descriptions.first().copied().unwrap_or_default()
    }

    /// The description of every path, parallel to [`paths`](Self::paths).
    pub fn descriptions(&self) -> &[&'static str] {
        &self.descriptions
    }

    /// Replaces the per-path descriptions; what `#[derive(SlashCommand)]`
    /// binds from `DESCRIPTIONS`. A slice that is not parallel to the
    /// paths is ignored.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_descriptions(self, descriptions: &'static [&'static str]) -> Self {
        if descriptions.len() == self.paths.len() && !descriptions.is_empty() {
            Self {
                descriptions: Arc::from(descriptions),
                ..self
            }
        } else {
            self
        }
    }

    /// Every Command Path the handler serves, in expansion order.
    pub fn paths(&self) -> &'static [&'static str] {
        self.paths
    }

    /// The option declarations of the handler's options struct, in field
    /// order; empty for a handler without one.
    pub fn options(&self) -> &'static [OptionDecl<'static>] {
        self.options
    }

    /// The declared metadata (`permissions`, `contexts`,
    /// `integration_types`, `nsfw`, `guilds`); [`CommandMetadata::new`]
    /// when none was given. It applies to the base command of every path
    /// (META-02).
    pub fn metadata(&self) -> &CommandMetadata {
        &self.metadata
    }

    /// The declared ephemerality: [`Ephemerality::Ephemeral`] with
    /// `#[command(.., response = ephemeral)]`, else
    /// [`Ephemerality::Public`] (INT-10).
    pub fn ephemerality(&self) -> Ephemerality {
        self.ephemerality
    }

    /// Whether the pipeline auto-defers the Command (INT-01): `false` when
    /// it is declared `#[command(.., auto_defer = false)]`, whose handler
    /// takes `CommandResponder<Manual>`, may open a modal as its first
    /// response and gets no auto-defer (INT-05).
    pub fn auto_defer(&self) -> bool {
        self.auto_defer
    }

    /// Replaces the metadata; what `#[command]` appends when a metadata
    /// argument is given.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_metadata(self, metadata: CommandMetadata) -> Self {
        Self { metadata, ..self }
    }

    /// Replaces the ephemerality; what `#[command(.., response = ..)]` appends.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_ephemerality(self, ephemerality: Ephemerality) -> Self {
        Self {
            ephemerality,
            ..self
        }
    }

    /// Sets the Extension types the handler requires; what
    /// `__RequireExtensions` calls.
    pub(crate) fn __with_requires(self, requires: &'static [crate::kind::__ExtType]) -> Self {
        Self { requires, ..self }
    }

    /// Sets the declared rate limits; what `#[derive(SlashCommand)]` binds
    /// from `RATE_LIMITS`.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_rate_limits(self, rate_limits: &'static [RateLimit]) -> Self {
        Self {
            rate_limits,
            ..self
        }
    }

    /// Attaches the struct's `#[on_rate_limited]` method.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_on_rate_limited(self, hook: __RateLimitedHook) -> Self {
        Self {
            on_rate_limited: Some(hook.0),
            ..self
        }
    }

    /// The declared rate limits, in declared order.
    pub(crate) fn rate_limits(&self) -> &'static [RateLimit] {
        self.rate_limits
    }

    /// The struct's `#[on_rate_limited]` method, if any.
    pub(crate) fn on_rate_limited(&self) -> Option<&Arc<RateLimitedFn>> {
        self.on_rate_limited.as_ref()
    }

    /// The Extension types its `Ext<T>` parameters require, in parameter
    /// order.
    pub(crate) fn requires(&self) -> &'static [crate::kind::__ExtType] {
        self.requires
    }

    /// The autocomplete handler linked to the option `option` of the
    /// handler's options struct, if any.
    pub(crate) fn autocomplete_for(&self, option: &str) -> Option<__AutocompleteMethod> {
        (self.autocomplete)(option)
    }

    /// Starts the handler for `ctx`; an [`UnknownCommandPath`] error when
    /// `ctx`'s path is not one of [`paths`](Self::paths).
    ///
    /// An input marked failed with [`CommandInput::with_error`] fails with
    /// that [`OptionError`](crate::OptionError) before any handler shape runs, so a caller's own
    /// decode failure is answered exactly like a slash decode failure.
    pub(crate) fn call(&self, ctx: CommandCtx, shared: Arc<Shared>) -> HandlerFuture {
        if let Some(error) = ctx.input().error() {
            return failed(HandlerError::new(error.clone()));
        }
        (self.call)(ctx, shared)
    }
}

/// A ready future failing with [`UnknownCommandPath`].
pub(crate) fn unknown<T: Send + 'static>(
    path: &str,
    command: &'static str,
) -> BoxFuture<'static, HandlerResult<T>> {
    failed(HandlerError::new(UnknownCommandPath {
        path: path.into(),
        command,
    }))
}

/// A ready future failing with `error`.
pub(crate) fn failed<T: Send + 'static>(
    error: HandlerError,
) -> BoxFuture<'static, HandlerResult<T>> {
    Box::pin(std::future::ready(Err(error)))
}

/// Decodes `O` from the context's input, then runs its validators; a
/// decode error comes first, so validators never see a partial struct. A
/// failed validator is a user condition carrying its key (FR-10): the user
/// gets that notice and no Error Sink sees it.
fn decode<O: Options>(ctx: &CommandCtx) -> Result<O, HandlerError> {
    let options = O::from_input(ctx.input()).map_err(HandlerError::new)?;
    options.validate(ctx).map_err(|invalid| {
        let key = invalid.key();
        HandlerError::user_condition(invalid, key)
    })?;
    Ok(options)
}

impl fmt::Debug for CommandHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandler")
            .field("name", &self.name)
            .field("pattern", &self.pattern)
            .field("paths", &self.paths)
            .field(
                "options",
                &self.options.iter().map(|o| o.name).collect::<Vec<_>>(),
            )
            .field("ephemerality", &self.ephemerality)
            .field("auto_defer", &self.auto_defer)
            .field("metadata", &self.metadata)
            .field("rate_limits", &self.rate_limits)
            .field("on_rate_limited", &self.on_rate_limited.is_some())
            .finish_non_exhaustive()
    }
}

/// Runs a started handler over a detached responder: a returned message
/// is validated and encoded, never sent; a responder error becomes the
/// handler error.
pub(crate) fn run_detached(
    start: impl FnOnce(Arc<Shared>) -> HandlerFuture,
    ephemerality: Ephemerality,
) -> BoxFuture<'static, HandlerResult<()>> {
    let shared = Shared::new(Transport::Detached, ephemerality, Limits::default());
    let fut = start(Arc::clone(&shared));
    Box::pin(async move {
        match fut.await?.0 {
            Response::Done => Ok(()),
            Response::Message(msg) => shared.finish(msg).await.map_err(HandlerError::new),
        }
    })
}

/// Runs `handler` for an invocation of `path` over the detached transport;
/// the test seam for `#[command]` (see `crate::__testing`).
pub(crate) fn call_command(
    handler: &CommandHandler,
    path: &'static str,
) -> BoxFuture<'static, HandlerResult<()>> {
    run_detached(
        |r| {
            let ctx =
                CommandCtx::detached(path, CommandInput::empty(path), handler.ephemerality, &r);
            handler.call(ctx, r)
        },
        handler.ephemerality,
    )
}

/// Decodes `json` (an interaction object) and runs `handler` for its path
/// over the detached transport; the test seam for options (see
/// `crate::__testing`).
pub(crate) fn call_command_json(
    handler: &CommandHandler,
    json: &[u8],
) -> BoxFuture<'static, HandlerResult<()>> {
    let input = match CommandInput::from_json(json) {
        Ok(input) => input,
        Err(error) => return failed(HandlerError::new(error)),
    };
    match handler.paths.iter().find(|p| **p == input.path()) {
        Some(path) => run_detached(
            |r| {
                let ctx = CommandCtx::detached(path, input, handler.ephemerality, &r);
                handler.call(ctx, r)
            },
            handler.ephemerality,
        ),
        None => unknown(input.path(), handler.name),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use noctis_ui::{Components, TextDisplay};

    use super::*;

    /// A one-text message.
    pub(crate) fn msg() -> Components {
        Components::new().push(TextDisplay::new("ok"))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum TestPath {
        A,
        B,
    }

    impl CommandPath for TestPath {
        const PATHS: &'static [&'static str] = &["t a", "t b"];
        fn from_path(path: &str) -> Option<Self> {
            match path {
                "t a" => Some(Self::A),
                "t b" => Some(Self::B),
                _ => None,
            }
        }
        fn as_str(self) -> &'static str {
            match self {
                Self::A => "t a",
                Self::B => "t b",
            }
        }
    }

    static SEEN: Mutex<Vec<TestPath>> = Mutex::new(Vec::new());

    async fn typed(
        ctx: CommandCtx,
        path: TestPath,
        _: CommandResponder,
    ) -> HandlerResult<Components> {
        assert_eq!(ctx.command_path(), path.as_str());
        SEEN.lock().map_err(|_| fmt::Error)?.push(path);
        Ok(msg())
    }

    async fn plain(_: CommandCtx, _: CommandResponder) -> HandlerResult<Components> {
        Ok(msg())
    }

    fn unknown_of(result: HandlerResult<()>) -> String {
        let Err(err) = result else {
            panic!("expected an error");
        };
        let unknown = err
            .as_error()
            .downcast_ref::<UnknownCommandPath>()
            .unwrap_or_else(|| panic!("not an UnknownCommandPath: {err}"));
        unknown.to_string()
    }

    #[tokio::test]
    async fn known_path_runs_with_its_variant() {
        let handler = CommandHandler::__new::<TestPath, _, _, _, _>("m::t", "t (a|b)", "d", typed);
        assert_eq!(handler.paths(), ["t a", "t b"]);
        assert_eq!(handler.pattern(), "t (a|b)");
        assert_eq!(handler.description(), "d");
        call_command(&handler, "t b")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(SEEN.lock().is_ok_and(|seen| seen.contains(&TestPath::B)));
        assert!(format!("{handler:?}").contains("\"t (a|b)\""));
        assert_eq!(handler.metadata(), &CommandMetadata::new());
        let handler = handler.__with_metadata(CommandMetadata::new().nsfw(true));
        assert_eq!(handler.metadata().nsfw, Some(true));
        assert!(format!("{handler:?}").contains("nsfw: Some(true)"));
    }

    #[tokio::test]
    async fn unknown_path_is_an_error() {
        let handler = CommandHandler::__new::<TestPath, _, _, _, _>("m::t", "t (a|b)", "d", typed);
        assert_eq!(
            unknown_of(call_command(&handler, "t c").await),
            "`t c` is not a path of the Command `m::t`"
        );
    }

    #[tokio::test]
    async fn plain_path_mismatch_is_an_error() {
        let handler = CommandHandler::__new_plain("m::ping", &["ping"], "d", plain);
        assert_eq!(handler.paths(), ["ping"]);
        assert_eq!(handler.pattern(), "ping");
        assert!(call_command(&handler, "ping").await.is_ok());
        assert_eq!(
            unknown_of(call_command(&handler, "pong").await),
            "`pong` is not a path of the Command `m::ping`"
        );
    }

    #[test]
    fn module_add_collects_commands() {
        let module = crate::Module::new("cfg")
            .__add(CommandHandler::__new::<TestPath, _, _, _, _>(
                "m::t", "t (a|b)", "d", typed,
            ))
            .__add(CommandHandler::__new_plain(
                "m::ping",
                &["ping"],
                "d",
                plain,
            ));
        let names: Vec<_> = module.commands().iter().map(CommandHandler::name).collect();
        assert_eq!(names, ["m::t", "m::ping"]);
        assert!(format!("{module:?}").contains("commands: [\"m::t\", \"m::ping\"]"));
    }

    #[test]
    fn ctx_debug_names_the_path() {
        let ctx = CommandCtx::new("a b");
        assert_eq!(
            format!("{ctx:?}"),
            "CommandCtx { command_path: \"a b\", module: \"\", shard: 0, guild_id: None, channel_id: None, user_id: Id<UserMarker>(1), user_locale: EnUs, guild_locale: EnUs, ephemerality: Public, .. }"
        );
    }

    #[test]
    fn ephemerality_is_declared_once() {
        let handler = CommandHandler::__new_plain("m::ping", &["ping"], "d", plain);
        assert_eq!(handler.ephemerality(), Ephemerality::Public);
        let handler = handler.__with_ephemerality(Ephemerality::Ephemeral);
        assert_eq!(handler.ephemerality(), Ephemerality::Ephemeral);
        assert!(format!("{handler:?}").contains("ephemerality: Ephemeral"));
    }

    /// Matrix 8 and 9 (Story 11.3 rows 6 and 48): the response locale
    /// follows the ephemerality, and every accessor reads the run's cell.
    #[test]
    fn response_locale_follows_visibility() {
        use noctis_rules::Locale::{De, Tr};
        let ctx = |user, guild, ephemerality| {
            CommandCtx::from_parts(CtxInner {
                locales: LocaleCell::new(Locales { user, guild }),
                ..CtxInner::detached_parts(ephemerality)
            })
        };
        let public = ctx(Tr, De, Ephemerality::Public);
        assert_eq!((public.user_locale(), public.guild_locale()), (Tr, De));
        assert_eq!(public.response_locale(), De);
        assert_eq!(ctx(Tr, De, Ephemerality::Ephemeral).response_locale(), Tr);
        assert!(format!("{public:?}").contains("user_locale: Tr, guild_locale: De"));
        public.0.locales.update(|_| Locales::both(Tr));
        assert_eq!(public.response_locale(), Tr);
        let detached = CommandCtx::new("a");
        assert_eq!(detached.response_locale(), Locale::EnUs);
        assert!(public.state::<u8>().is_none());
        let _ = public.http();
    }

    mod with_options {
        use std::sync::Mutex;

        use noctis_rules::MessageKey;
        use noctis_rules::command::{OptionDecl, OptionType};

        use super::*;
        use crate::options::__options::{optional, required};
        use crate::options::{InvalidOption, OptionError};

        #[derive(Debug, Clone, PartialEq)]
        struct W {
            days: i64,
            reason: Option<i64>,
        }

        fn positive(v: i64) -> Result<(), MessageKey> {
            if v > 0 {
                Ok(())
            } else {
                Err(MessageKey::new("warn.days.positive"))
            }
        }

        impl Options for W {
            const OPTIONS: &'static [OptionDecl<'static>] = &[
                OptionDecl::new("days", OptionType::Integer, "d").required(true),
                OptionDecl::new("reason", OptionType::Integer, "d"),
            ];
            type Partial = crate::autocomplete::NoPartial<Self>;
            fn from_input(input: &CommandInput) -> Result<Self, OptionError> {
                Ok(Self {
                    days: required(input, "days")?,
                    reason: optional(input, "reason")?,
                })
            }
            fn validate(&self, _: &CommandCtx) -> Result<(), InvalidOption> {
                positive(self.days).map_err(|k| InvalidOption::__new("days", k))?;
                if let Some(v) = self.reason {
                    positive(v).map_err(|k| InvalidOption::__new("reason", k))?;
                }
                Ok(())
            }
        }

        static GOT: Mutex<Vec<W>> = Mutex::new(Vec::new());

        async fn warn(_: CommandCtx, _: CommandResponder, w: W) -> HandlerResult<Components> {
            GOT.lock().map_err(|_| fmt::Error)?.push(w);
            Ok(msg())
        }

        async fn typed(
            _: CommandCtx,
            path: TestPath,
            _: CommandResponder,
            w: W,
        ) -> HandlerResult<Components> {
            assert_eq!(path, TestPath::B);
            GOT.lock().map_err(|_| fmt::Error)?.push(w);
            Ok(msg())
        }

        fn json(path: &str, options: &str) -> Vec<u8> {
            let mut words = path.split(' ');
            let base = words.next().unwrap_or_default();
            let mut opts = options.to_owned();
            for word in words.rev() {
                opts = format!(r#"[{{"type":1,"name":"{word}","options":{opts}}}]"#);
            }
            format!(r#"{{"type":2,"data":{{"name":"{base}","options":{opts}}}}}"#).into_bytes()
        }

        fn seen(w: &W) -> bool {
            GOT.lock().is_ok_and(|g| g.contains(w))
        }

        #[tokio::test]
        async fn decodes_validates_and_runs() {
            let h = CommandHandler::__new_plain_with_options("m::warn", &["warn"], "d", warn);
            assert_eq!(h.options().len(), 2);
            assert!(format!("{h:?}").contains("options: [\"days\", \"reason\"]"));
            let ok = call_command_json(
                &h,
                &json(
                    "warn",
                    r#"[{"name":"days","type":4,"value":5},{"name":"foo","type":3,"value":"x"}]"#,
                ),
            )
            .await;
            assert!(ok.is_ok(), "{ok:?}");
            assert!(seen(&W {
                days: 5,
                reason: None
            }));

            let err = call_command_json(
                &h,
                &json("warn", r#"[{"name":"days","type":4,"value":-1}]"#),
            )
            .await
            .unwrap_err();
            let invalid = err
                .as_error()
                .downcast_ref::<InvalidOption>()
                .unwrap_or_else(|| panic!("{err}"));
            assert_eq!(invalid.option(), "days");
            assert_eq!(invalid.key(), MessageKey::new("warn.days.positive"));
            assert_eq!(
                err.to_string(),
                "option `days` failed validation: warn.days.positive"
            );
            assert!(!seen(&W {
                days: -1,
                reason: None
            }));

            // Validators run in declaration order; the first failure wins.
            let err = call_command_json(&h, &json("warn", r#"[{"name":"reason","type":4,"value":-3},{"name":"days","type":4,"value":-2}]"#))
                .await
                .unwrap_err();
            let invalid = err
                .as_error()
                .downcast_ref::<InvalidOption>()
                .unwrap_or_else(|| panic!("{err}"));
            assert_eq!(invalid.option(), "days");

            // Decode errors come before validators.
            let err = call_command_json(
                &h,
                &json("warn", r#"[{"name":"reason","type":4,"value":-3}]"#),
            )
            .await
            .unwrap_err();
            assert_eq!(
                err.as_error().downcast_ref::<OptionError>(),
                Some(&OptionError::Missing {
                    option: "days".into()
                })
            );

            let err = call_command_json(&h, &json("kick", "[]"))
                .await
                .unwrap_err();
            assert!(
                err.as_error()
                    .downcast_ref::<UnknownCommandPath>()
                    .is_some()
            );

            let err = call_command_json(&h, b"{").await.unwrap_err();
            assert!(matches!(
                err.as_error().downcast_ref::<OptionError>(),
                Some(OptionError::Malformed { .. })
            ));
        }

        #[tokio::test]
        async fn pattern_with_options() {
            let h = CommandHandler::__new_with_options::<TestPath, W, _, _, _, _>(
                "m::t", "t (a|b)", "d", typed,
            );
            assert_eq!(h.options()[0].name, "days");
            let ok = call_command_json(&h, &json("t b", r#"[{"name":"days","type":4,"value":7}]"#))
                .await;
            assert!(ok.is_ok(), "{ok:?}");
            assert!(seen(&W {
                days: 7,
                reason: None
            }));
            let err = call_command_json(&h, &json("t c", "[]")).await.unwrap_err();
            assert!(
                err.as_error()
                    .downcast_ref::<UnknownCommandPath>()
                    .is_some()
            );
        }

        #[tokio::test]
        async fn no_options_handler_ignores_options() {
            let h = CommandHandler::__new_plain("m::ping", &["ping"], "d", plain);
            assert!(h.options().is_empty());
            let ok =
                call_command_json(&h, &json("ping", r#"[{"name":"x","type":4,"value":1}]"#)).await;
            assert!(ok.is_ok(), "{ok:?}");
        }
    }
}
