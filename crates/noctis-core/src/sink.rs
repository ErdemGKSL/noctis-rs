//! Error Sinks: where handler errors, panics and overflow drops go, one sink
//! per [`ErrorOrigin`] (FR-22, AD-13, NFR-6).

use std::error::Error;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::HandlerError;
use crate::worker::panic_message;

/// Where a reported failure came from; each origin has its own
/// [`ErrorSink`] (`AppBuilder::error_sink`).
///
/// ```
/// use noctis_core::ErrorOrigin;
///
/// assert_eq!(ErrorOrigin::Event.as_str(), "event");
/// assert_eq!(ErrorOrigin::Component.to_string(), "component");
/// assert_eq!(ErrorOrigin::Hook.as_str(), "hook");
/// assert_eq!(ErrorOrigin::ALL.len(), 6);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorOrigin {
    /// A slash, user or message command handler.
    Command,
    /// A component interaction or modal submit handler.
    Component,
    /// An autocomplete handler.
    Autocomplete,
    /// A gateway event handler, or a gateway event dropped by a full
    /// overflow.
    Event,
    /// An image render.
    Render,
    /// A Lifecycle Hook (`#[derive(Hook)]`, `noctis::lifecycle`).
    Hook,
}

impl ErrorOrigin {
    /// Every origin, in declaration order.
    pub const ALL: [ErrorOrigin; 6] = [
        Self::Command,
        Self::Component,
        Self::Autocomplete,
        Self::Event,
        Self::Render,
        Self::Hook,
    ];

    /// The lowercase name: `"command"`, `"component"`, `"autocomplete"`,
    /// `"event"`, `"render"` or `"hook"` (also the `kind` metric label
    /// value).
    ///
    /// ```
    /// assert_eq!(noctis_core::ErrorOrigin::Render.as_str(), "render");
    /// ```
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Component => "component",
            Self::Autocomplete => "autocomplete",
            Self::Event => "event",
            Self::Render => "render",
            Self::Hook => "hook",
        }
    }

    /// The index of this origin in [`ALL`](Self::ALL).
    const fn index(self) -> usize {
        match self {
            Self::Command => 0,
            Self::Component => 1,
            Self::Autocomplete => 2,
            Self::Event => 3,
            Self::Render => 4,
            Self::Hook => 5,
        }
    }
}

impl fmt::Display for ErrorOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happened.
///
/// ```
/// use noctis_core::ReportKind;
///
/// fn describe(kind: ReportKind<'_>) -> String {
///     match kind {
///         ReportKind::Error(error) => format!("failed: {error}"),
///         ReportKind::Panic(message) => format!("panicked: {message}"),
///         ReportKind::Dropped => "dropped".to_owned(),
///         ReportKind::Cancelled => "cancelled at shutdown".to_owned(),
///         _ => "other".to_owned(),
///     }
/// }
/// assert_eq!(describe(ReportKind::Panic("boom")), "panicked: boom");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum ReportKind<'a> {
    /// A handler (or hook) returned `Err`.
    Error(&'a HandlerError),
    /// A handler panicked; the panic's `&str`/`String` message, or
    /// `"<non-string panic>"`.
    Panic(&'a str),
    /// A non-droppable gateway event was dropped because the event
    /// overflow was full.
    Dropped,
    /// The handler was still running, or still queued, when the shutdown
    /// grace period ended.
    Cancelled,
}

/// One failure handed to an [`ErrorSink`]. Only noctis constructs it.
///
/// It never carries a payload, a token or an interaction token.
///
/// ```
/// use noctis_core::{ErrorReport, ReportKind};
///
/// fn line(report: &ErrorReport<'_>) -> String {
///     let what = match report.kind {
///         ReportKind::Error(_) => "error",
///         ReportKind::Panic(_) => "panic",
///         _ => "other",
///     };
///     format!("{} {what} in {:?}", report.origin, report.handler)
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct ErrorReport<'a> {
    /// Where the failure came from.
    pub origin: ErrorOrigin,
    /// What happened.
    pub kind: ReportKind<'a>,
    /// The App id ([`AppId`](crate::AppId)); `"default"` only when none is
    /// set or derivable.
    pub app: &'a str,
    /// The shard that received the triggering event or interaction.
    pub shard: Option<u32>,
    /// The guild, when there is one.
    pub guild_id: Option<Id<GuildMarker>>,
    /// The handler's Module id.
    pub module: Option<&'a str>,
    /// The handler, command or Route path.
    pub handler: Option<&'a str>,
    /// The Discord event name, e.g. `"MESSAGE_DELETE"`.
    pub event_type: Option<&'a str>,
    /// The run's frozen [`Extensions`](crate::Extensions): `Some` (possibly
    /// empty) for every Command, context-menu, `Dispatcher`, component,
    /// modal or event handler run (stopped and failed ones included), `None`
    /// for overflow drops, autocomplete and Expired Handler chain runs.
    pub extensions: Option<&'a crate::Extensions>,
    /// The Middleware that failed or panicked, when one did (its name, the
    /// type name of its function).
    pub middleware: Option<&'a str>,
}

/// Receives the [`ErrorReport`]s of one or more [`ErrorOrigin`]s.
///
/// `report` is synchronous: it runs on the worker task that ran the
/// handler, or on the shard task for an overflow drop, so it must not
/// block. For I/O (paging, a webhook, a database) spawn a task or forward
/// the copied fields to a channel. A panicking sink is caught and logged
/// (`noctis::error_sink`); it never takes a worker or shard down.
///
/// ```
/// use noctis_core::{ErrorReport, ErrorSink, ReportKind};
///
/// struct Alert;
///
/// impl ErrorSink for Alert {
///     fn report(&self, report: ErrorReport<'_>) {
///         if let ReportKind::Panic(message) = report.kind {
///             tracing::error!(origin = %report.origin, message, "page on-call");
///         }
///     }
/// }
/// ```
pub trait ErrorSink: Send + Sync + 'static {
    /// Handles one report; must not block.
    fn report(&self, report: ErrorReport<'_>);
}

/// One sink shared by several origins.
///
/// ```
/// use std::sync::Arc;
///
/// use noctis_core::{App, ErrorOrigin, LogSink};
///
/// let shared = Arc::new(LogSink);
/// let mut builder = App::builder();
/// for origin in ErrorOrigin::ALL {
///     builder.error_sink(origin, Arc::clone(&shared));
/// }
/// ```
impl<S: ErrorSink + ?Sized> ErrorSink for Arc<S> {
    fn report(&self, report: ErrorReport<'_>) {
        (**self).report(report);
    }
}

/// The default sink: logs every report at `error`, never a payload.
///
/// The target is `noctis::<origin>` (e.g. `noctis::event`). Fields: `app`,
/// and `shard`, `guild_id`, `module`, `handler` and `event_type` when
/// known, plus `error` (the error and its `source()` chain joined with
/// `": "`, at most 16 links) or `panic`, and `middleware` when a
/// Middleware failed (never an Extension value). Messages: `"<origin> handler
/// failed"`, `"<origin> handler panicked"`, `"<origin> handler cancelled
/// at shutdown"` and `"event overflow is full; event dropped"`.
///
/// ```
/// use noctis_core::{App, ErrorOrigin, LogSink};
///
/// let mut builder = App::builder();
/// builder.error_sink(ErrorOrigin::Event, LogSink);
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct LogSink;

/// The most `source()` links [`LogSink`] prints.
const MAX_CHAIN: usize = 16;

/// An error and its `source()` chain, joined with `": "`.
struct Chain<'a>(&'a (dyn Error + 'static));

impl fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut next = Some(self.0);
        for index in 0..MAX_CHAIN {
            let Some(error) = next else { break };
            if index > 0 {
                f.write_str(": ")?;
            }
            fmt::Display::fmt(error, f)?;
            next = error.source();
        }
        Ok(())
    }
}

/// Logs `$report` with target `$target` and origin name `$origin`.
macro_rules! log_report {
    ($report:ident, $target:literal, $origin:literal) => {{
        let app = $report.app;
        let shard = $report.shard;
        let guild_id = $report.guild_id.map(Id::get);
        let module = $report.module;
        let handler = $report.handler;
        let event_type = $report.event_type;
        let middleware = $report.middleware;
        match $report.kind {
            ReportKind::Error(error) => tracing::error!(
                target: $target,
                app,
                shard,
                guild_id,
                module,
                handler,
                event_type,
                middleware,
                error = %Chain(error.as_error()),
                concat!($origin, " handler failed")
            ),
            ReportKind::Panic(panic) => tracing::error!(
                target: $target,
                app,
                shard,
                guild_id,
                module,
                handler,
                event_type,
                middleware,
                panic,
                concat!($origin, " handler panicked")
            ),
            ReportKind::Cancelled => tracing::error!(
                target: $target,
                app,
                shard,
                guild_id,
                module,
                handler,
                event_type,
                middleware,
                concat!($origin, " handler cancelled at shutdown")
            ),
            ReportKind::Dropped => tracing::error!(
                target: $target,
                app,
                shard,
                guild_id,
                module,
                handler,
                event_type,
                "event overflow is full; event dropped"
            ),
        }
    }};
}

impl ErrorSink for LogSink {
    fn report(&self, report: ErrorReport<'_>) {
        match report.origin {
            ErrorOrigin::Command => log_report!(report, "noctis::command", "command"),
            ErrorOrigin::Component => log_report!(report, "noctis::component", "component"),
            ErrorOrigin::Autocomplete => {
                log_report!(report, "noctis::autocomplete", "autocomplete");
            }
            ErrorOrigin::Event => log_report!(report, "noctis::event", "event"),
            ErrorOrigin::Render => log_report!(report, "noctis::render", "render"),
            ErrorOrigin::Hook => log_report!(report, "noctis::hook", "hook"),
        }
    }
}

/// The App's sinks, one per origin; `None` routes to [`LogSink`].
#[derive(Clone, Default)]
pub(crate) struct ErrorSinks {
    custom: [Option<Arc<dyn ErrorSink>>; ErrorOrigin::ALL.len()],
}

impl ErrorSinks {
    /// Routes `origin` to `sink`; the last call wins.
    pub(crate) fn set(&mut self, origin: ErrorOrigin, sink: Arc<dyn ErrorSink>) {
        self.custom[origin.index()] = Some(sink);
    }

    /// Whether `origin` has a custom sink.
    pub(crate) fn is_custom(&self, origin: ErrorOrigin) -> bool {
        self.custom[origin.index()].is_some()
    }

    /// The origins with a custom sink, in [`ErrorOrigin::ALL`] order.
    pub(crate) fn custom_origins(&self) -> Vec<ErrorOrigin> {
        ErrorOrigin::ALL
            .into_iter()
            .filter(|origin| self.is_custom(*origin))
            .collect()
    }

    /// Hands `report` to its origin's sink, catching a panicking sink.
    pub(crate) fn report(&self, report: ErrorReport<'_>) {
        let origin = report.origin;
        let sink = self.custom[origin.index()].as_deref();
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| match sink {
            Some(sink) => sink.report(report),
            None => LogSink.report(report),
        }));
        if let Err(panic) = outcome {
            tracing::error!(
                target: "noctis::error_sink",
                origin = origin.as_str(),
                panic = panic_message(&*panic),
                "error sink panicked"
            );
        }
    }
}

impl fmt::Debug for ErrorSinks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.custom_origins()).finish()
    }
}

/// Reports overflow drops to the App's `Event` sink.
pub(crate) struct EventDropObserver {
    pub(crate) app: Arc<str>,
    pub(crate) sinks: ErrorSinks,
}

impl noctis_gateway::DropObserver for EventDropObserver {
    fn overflow_dropped(&self, shard: u32, event: &twilight_model::gateway::event::Event) {
        self.sinks.report(ErrorReport {
            origin: ErrorOrigin::Event,
            kind: ReportKind::Dropped,
            app: &self.app,
            shard: Some(shard),
            guild_id: event.guild_id(),
            module: None,
            handler: None,
            event_type: event.kind().name(),
            extensions: None,
            middleware: None,
        });
    }
}

#[cfg(test)]
mod tests;
