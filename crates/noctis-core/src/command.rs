//! Slash Commands: the [`CommandHandler`] descriptor `#[command]` expands to,
//! the [`CommandCtx`] handlers receive, the [`CommandPath`] trait of the
//! generated path enums and [`UnknownCommandPath`] (FR-7, FR-8, AD-8, AD-26).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::handler::{HandlerError, HandlerResult};

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
/// Cheap to clone (one `Arc`). Today it carries only the invoked
/// [`command_path`](Self::command_path); the interaction accessors arrive with
/// the Command pipeline.
#[derive(Clone)]
pub struct CommandCtx(Arc<CtxInner>);

struct CtxInner {
    path: &'static str,
}

impl CommandCtx {
    /// A context for an invocation of `path`.
    pub(crate) fn new(path: &'static str) -> Self {
        Self(Arc::new(CtxInner { path }))
    }

    /// The invoked Command Path, words joined by one ASCII space
    /// (`"config set prefix"`).
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
/// Opaque and cheap to clone. `Debug` prints the name, the pattern and the
/// paths. Annotating `async fn config` with `#[command(..)]` gives a
/// `fn config()` that returns its `CommandHandler`.
#[derive(Clone)]
pub struct CommandHandler {
    name: &'static str,
    pattern: &'static str,
    description: &'static str,
    paths: &'static [&'static str],
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

    /// Starts the handler for `ctx`; an [`UnknownCommandPath`] error when
    /// `ctx`'s path is not one of [`paths`](Self::paths).
    pub(crate) fn call(&self, ctx: CommandCtx) -> BoxFuture<'static, HandlerResult<()>> {
        (self.call)(ctx)
    }
}

/// A ready future failing with [`UnknownCommandPath`].
fn unknown(path: &str, command: &'static str) -> BoxFuture<'static, HandlerResult<()>> {
    let error = HandlerError::new(UnknownCommandPath {
        path: path.into(),
        command,
    });
    Box::pin(std::future::ready(Err(error)))
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "the description is user text and the closure has no Debug"
)]
impl fmt::Debug for CommandHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandler")
            .field("name", &self.name)
            .field("pattern", &self.pattern)
            .field("paths", &self.paths)
            .finish()
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
}
