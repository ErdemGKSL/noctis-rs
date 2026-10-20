//! Fixtures shared by the core unit tests.

use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::MessageDelete;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::events::{EventCtx, EventHandler, EventPayload};
use crate::{ErrorOrigin, ErrorReport, ErrorSink, HandlerResult, ReportKind};

/// A handler of `P` that does nothing.
pub(crate) fn noop<P: EventPayload>(name: &'static str) -> EventHandler {
    EventHandler::__new(name, false, false, |_: EventCtx, _: P| async { Ok(()) })
}

/// A handler of `P` with flags that does nothing.
pub(crate) fn noop_with<P: EventPayload>(
    name: &'static str,
    ordered: bool,
    droppable: bool,
) -> EventHandler {
    EventHandler::__new(name, ordered, droppable, |_: EventCtx, _: P| async {
        Ok(())
    })
}

/// A handler of `P` running `f`.
pub(crate) fn handler<P, F, Fut>(name: &'static str, ordered: bool, f: F) -> EventHandler
where
    P: EventPayload,
    F: Fn(EventCtx, P) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<()>> + Send + 'static,
{
    EventHandler::__new(name, ordered, false, f)
}

/// A `MESSAGE_DELETE` of message `id` in `guild`.
pub(crate) fn message_delete(id: u64, guild: Option<u64>) -> Event {
    Event::MessageDelete(MessageDelete {
        channel_id: Id::new(1),
        guild_id: guild.map(Id::<GuildMarker>::new),
        id: Id::new(id),
    })
}

/// Appends every log line to a shared buffer.
#[derive(Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub(crate) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    /// The lines containing every needle.
    pub(crate) fn lines_with(&self, needles: &[&str]) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| needles.iter().all(|n| line.contains(n)))
            .map(str::to_owned)
            .collect()
    }

    /// A TRACE-level subscriber writing here; set it with
    /// `tracing::subscriber::set_default`.
    pub(crate) fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The owned fields of one report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) origin: ErrorOrigin,
    /// `error:<display>`, `panic:<message>`, `dropped` or `cancelled`.
    pub(crate) kind: String,
    pub(crate) app: String,
    pub(crate) shard: Option<u32>,
    pub(crate) guild_id: Option<u64>,
    pub(crate) module: Option<String>,
    pub(crate) handler: Option<String>,
    pub(crate) event_type: Option<String>,
}

/// A sink recording every report.
#[derive(Default)]
pub(crate) struct Recording(pub(crate) Mutex<Vec<Seen>>);

impl Recording {
    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.0.lock().unwrap().clone()
    }
}

impl ErrorSink for Recording {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => format!("error:{error}"),
            ReportKind::Panic(message) => format!("panic:{message}"),
            ReportKind::Dropped => "dropped".to_owned(),
            ReportKind::Cancelled => "cancelled".to_owned(),
        };
        self.0.lock().unwrap().push(Seen {
            origin: report.origin,
            kind,
            app: report.app.to_owned(),
            shard: report.shard,
            guild_id: report.guild_id.map(Id::get),
            module: report.module.map(str::to_owned),
            handler: report.handler.map(str::to_owned),
            event_type: report.event_type.map(str::to_owned),
        });
    }
}
