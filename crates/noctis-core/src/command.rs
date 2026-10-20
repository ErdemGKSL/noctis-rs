//! Slash Commands: the [`CommandHandler`] descriptor `#[command]` expands to,
//! the [`CommandCtx`] handlers receive, the [`CommandPath`] trait of the
//! generated path enums and [`UnknownCommandPath`] (FR-7, FR-8, AD-8, AD-26).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use noctis_rules::command::OptionDecl;

use crate::handler::{HandlerError, HandlerResult};
use crate::metadata::CommandMetadata;
use crate::options::{CommandInput, Options};

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

/// The context a Command handler receives.
///
/// Cheap to clone (one `Arc`). Today it carries the invoked
/// [`command_path`](Self::command_path) and the decoded interaction input
/// the options struct is built from; the interaction accessors arrive with
/// the Command pipeline.
#[derive(Clone)]
pub struct CommandCtx(Arc<CtxInner>);

struct CtxInner {
    path: &'static str,
    input: CommandInput,
}

impl CommandCtx {
    /// A context for an invocation of `path` without options.
    pub(crate) fn new(path: &'static str) -> Self {
        Self::with_input(path, CommandInput::empty(path))
    }

    /// A context for an invocation of `path` with the decoded `input`.
    pub(crate) fn with_input(path: &'static str, input: CommandInput) -> Self {
        Self(Arc::new(CtxInner { path, input }))
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
}

impl fmt::Debug for CommandCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandCtx")
            .field("command_path", &self.0.path)
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

/// The erased handler call.
type ErasedCommand = dyn Fn(CommandCtx) -> BoxFuture<'static, HandlerResult<()>> + Send + Sync;

/// One slash Command handler, as `#[command]` builds it; add it to a
/// [`Module`](crate::Module).
///
/// Opaque and cheap to clone. `Debug` prints the name, the pattern, the
/// paths, the option names and the metadata. Annotating `async fn config` with `#[command(..)]` gives a
/// `fn config()` that returns its `CommandHandler`.
#[derive(Clone)]
pub struct CommandHandler {
    name: &'static str,
    pattern: &'static str,
    description: &'static str,
    paths: &'static [&'static str],
    options: &'static [OptionDecl<'static>],
    metadata: CommandMetadata,
    call: Arc<ErasedCommand>,
}

impl CommandHandler {
    /// A handler of the Command Pattern `pattern` whose paths are `P`'s;
    /// what `#[command]` expands to for a pattern with a group.
    #[doc(hidden)]
    pub fn __new<P, F, Fut>(
        name: &'static str,
        pattern: &'static str,
        description: &'static str,
        f: F,
    ) -> Self
    where
        P: CommandPath,
        F: Fn(CommandCtx, P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: CommandCtx| -> BoxFuture<'static, HandlerResult<()>> {
            match P::from_path(ctx.command_path()) {
                Some(path) => Box::pin(f(ctx, path)),
                None => unknown(ctx.command_path(), name),
            }
        };
        Self {
            name,
            pattern,
            description,
            paths: P::PATHS,
            options: &[],
            metadata: CommandMetadata::new(),
            call: Arc::new(call),
        }
    }

    /// Like [`__new`](Self::__new) for a handler that also takes the options
    /// struct `O`: the path is checked, `O` is decoded and validated, then
    /// the handler runs.
    #[doc(hidden)]
    pub fn __new_with_options<P, O, F, Fut>(
        name: &'static str,
        pattern: &'static str,
        description: &'static str,
        f: F,
    ) -> Self
    where
        P: CommandPath,
        O: Options,
        F: Fn(CommandCtx, P, O) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: CommandCtx| -> BoxFuture<'static, HandlerResult<()>> {
            let Some(path) = P::from_path(ctx.command_path()) else {
                return unknown(ctx.command_path(), name);
            };
            match decode::<O>(&ctx) {
                Ok(options) => Box::pin(f(ctx, path, options)),
                Err(error) => failed(error),
            }
        };
        Self {
            name,
            pattern,
            description,
            paths: P::PATHS,
            options: O::OPTIONS,
            metadata: CommandMetadata::new(),
            call: Arc::new(call),
        }
    }

    /// A handler of the single Command Path `paths[0]`; what `#[command]`
    /// expands to for a pattern without a group.
    #[doc(hidden)]
    pub fn __new_plain<F, Fut>(
        name: &'static str,
        paths: &'static [&'static str],
        description: &'static str,
        f: F,
    ) -> Self
    where
        F: Fn(CommandCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: CommandCtx| -> BoxFuture<'static, HandlerResult<()>> {
            if paths.contains(&ctx.command_path()) {
                Box::pin(f(ctx))
            } else {
                unknown(ctx.command_path(), name)
            }
        };
        Self {
            name,
            pattern: paths.first().copied().unwrap_or_default(),
            description,
            paths,
            options: &[],
            metadata: CommandMetadata::new(),
            call: Arc::new(call),
        }
    }

    /// Like [`__new_plain`](Self::__new_plain) for a handler that also takes
    /// the options struct `O`.
    #[doc(hidden)]
    pub fn __new_plain_with_options<O, F, Fut>(
        name: &'static str,
        paths: &'static [&'static str],
        description: &'static str,
        f: F,
    ) -> Self
    where
        O: Options,
        F: Fn(CommandCtx, O) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: CommandCtx| -> BoxFuture<'static, HandlerResult<()>> {
            if !paths.contains(&ctx.command_path()) {
                return unknown(ctx.command_path(), name);
            }
            match decode::<O>(&ctx) {
                Ok(options) => Box::pin(f(ctx, options)),
                Err(error) => failed(error),
            }
        };
        Self {
            name,
            pattern: paths.first().copied().unwrap_or_default(),
            description,
            paths,
            options: O::OPTIONS,
            metadata: CommandMetadata::new(),
            call: Arc::new(call),
        }
    }

    /// The handler's path (`module_path!()::name`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The Command Path or Pattern as written in `#[command("…")]`.
    pub fn pattern(&self) -> &'static str {
        self.pattern
    }

    /// The literal `description = "…"` (empty when none was given, which
    /// does not compile).
    pub fn description(&self) -> &'static str {
        self.description
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

    /// Replaces the metadata; what `#[command]` appends when a metadata
    /// argument is given.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_metadata(self, metadata: CommandMetadata) -> Self {
        Self { metadata, ..self }
    }

    /// Starts the handler for `ctx`; an [`UnknownCommandPath`] error when
    /// `ctx`'s path is not one of [`paths`](Self::paths).
    pub(crate) fn call(&self, ctx: CommandCtx) -> BoxFuture<'static, HandlerResult<()>> {
        (self.call)(ctx)
    }
}

/// A ready future failing with [`UnknownCommandPath`].
pub(crate) fn unknown(path: &str, command: &'static str) -> BoxFuture<'static, HandlerResult<()>> {
    failed(HandlerError::new(UnknownCommandPath {
        path: path.into(),
        command,
    }))
}

/// A ready future failing with `error`.
pub(crate) fn failed(error: HandlerError) -> BoxFuture<'static, HandlerResult<()>> {
    Box::pin(std::future::ready(Err(error)))
}

/// Decodes `O` from the context's input, then runs its validators; a
/// decode error comes first, so validators never see a partial struct.
fn decode<O: Options>(ctx: &CommandCtx) -> Result<O, HandlerError> {
    let options = O::from_input(ctx.input()).map_err(HandlerError::new)?;
    options.validate(ctx).map_err(HandlerError::new)?;
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
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

/// Runs `handler` for an invocation of `path`; the test seam for
/// `#[command]` (see `crate::__testing`).
pub(crate) fn call_command(
    handler: &CommandHandler,
    path: &'static str,
) -> BoxFuture<'static, HandlerResult<()>> {
    handler.call(CommandCtx::new(path))
}

/// Decodes `json` (an interaction object) and runs `handler` for its path;
/// the test seam for options (see `crate::__testing`).
pub(crate) fn call_command_json(
    handler: &CommandHandler,
    json: &[u8],
) -> BoxFuture<'static, HandlerResult<()>> {
    let input = match CommandInput::from_json(json) {
        Ok(input) => input,
        Err(error) => return failed(HandlerError::new(error)),
    };
    match handler.paths.iter().find(|p| **p == input.path()) {
        Some(path) => handler.call(CommandCtx::with_input(path, input)),
        None => unknown(input.path(), handler.name),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

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

    async fn typed(ctx: CommandCtx, path: TestPath) -> HandlerResult<()> {
        assert_eq!(ctx.command_path(), path.as_str());
        SEEN.lock().map_err(|_| fmt::Error)?.push(path);
        Ok(())
    }

    async fn plain(_: CommandCtx) -> HandlerResult<()> {
        Ok(())
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
        let handler = CommandHandler::__new::<TestPath, _, _>("m::t", "t (a|b)", "d", typed);
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
        let handler = CommandHandler::__new::<TestPath, _, _>("m::t", "t (a|b)", "d", typed);
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
            .add(CommandHandler::__new::<TestPath, _, _>(
                "m::t", "t (a|b)", "d", typed,
            ))
            .add(CommandHandler::__new_plain(
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
            "CommandCtx { command_path: \"a b\", .. }"
        );
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

        async fn warn(_: CommandCtx, w: W) -> HandlerResult<()> {
            GOT.lock().map_err(|_| fmt::Error)?.push(w);
            Ok(())
        }

        async fn typed(_: CommandCtx, path: TestPath, w: W) -> HandlerResult<()> {
            assert_eq!(path, TestPath::B);
            GOT.lock().map_err(|_| fmt::Error)?.push(w);
            Ok(())
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
            let h = CommandHandler::__new_with_options::<TestPath, W, _, _>(
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
