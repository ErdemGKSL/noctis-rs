//! Fixtures shared by the core unit tests.

use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::MessageDelete;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::HandlerResult;
use crate::events::{EventCtx, EventHandler, EventPayload};

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
