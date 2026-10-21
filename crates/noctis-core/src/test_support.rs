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

/// A one-text V2 message body.
pub(crate) fn msg() -> noctis_ui::Components {
    noctis_ui::Components::new().push(noctis_ui::TextDisplay::new("ok"))
}

/// A chat-input handler named `name` serving `paths`, described `description`.
pub(crate) fn command(
    name: &'static str,
    paths: &'static [&'static str],
    description: &'static str,
) -> crate::CommandHandler {
    crate::CommandHandler::__new_plain(
        name,
        paths,
        description,
        |_: crate::CommandCtx, _: crate::CommandResponder| async { Ok(msg()) },
    )
}

/// [`command`] with `metadata`.
pub(crate) fn command_with(
    name: &'static str,
    paths: &'static [&'static str],
    metadata: crate::CommandMetadata,
) -> crate::CommandHandler {
    command(name, paths, "d").__with_metadata(metadata)
}

/// A `'static` path (or path list) built at run time.
pub(crate) fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A `'static` one-path list.
pub(crate) fn leak_paths(path: &str) -> &'static [&'static str] {
    Box::leak(vec![leak(path.to_owned())].into_boxed_slice())
}

/// A `USER` command handler named `name` for the menu `menu`.
pub(crate) fn user_menu(
    name: &'static str,
    menu: &'static str,
    metadata: crate::CommandMetadata,
) -> crate::ContextMenuHandler {
    crate::ContextMenuHandler::__new_user(
        name,
        menu,
        metadata,
        |_: crate::CommandCtx, _: noctis_ui::ResolvedUser, _: crate::CommandResponder| async {
            Ok(msg())
        },
    )
}

/// A `MESSAGE` command handler named `name` for the menu `menu`.
pub(crate) fn message_menu(
    name: &'static str,
    menu: &'static str,
    metadata: crate::CommandMetadata,
) -> crate::ContextMenuHandler {
    crate::ContextMenuHandler::__new_message(
        name,
        menu,
        metadata,
        |_: crate::CommandCtx, _: noctis_ui::ResolvedMessage, _: crate::CommandResponder| async {
            Ok(msg())
        },
    )
}

/// A hand-written component and modal Route struct with one `u64` field
/// (`item`): what a kind derive implements. A modal submission also needs
/// the text input `bio`.
macro_rules! test_route {
    ($name:ident, $id:literal, $shape:expr, $schema:literal) => {
        #[derive(Debug)]
        pub(crate) struct $name {
            pub(crate) item: u64,
        }

        impl noctis_codec::Payload for $name {
            const MAX_ENCODED_LEN: Option<usize> = Some(11);

            fn encode_fields(&self, out: &mut noctis_codec::FieldWriter<'_>) -> std::fmt::Result {
                out.field(&self.item)
            }
        }

        impl noctis_codec::PayloadDecode<'_> for $name {
            fn decode_fields(
                f: &mut noctis_codec::FieldReader<'_>,
            ) -> Result<Self, noctis_codec::CodecError> {
                Ok(Self { item: f.field()? })
            }
        }

        impl noctis_codec::Route for $name {
            const ID: noctis_codec::RouteId = noctis_codec::RouteId::new($id);
            const SHAPE: noctis_codec::RouteShape = $shape;
            const SCHEMA: u8 = $schema;
            type Payload = Self;
        }

        impl noctis_ui::Handled for $name {}

        impl noctis_ui::ModalForm for $name {
            fn from_submission(
                submission: &noctis_ui::ModalSubmission,
            ) -> Result<Self, noctis_ui::SubmitError> {
                let this = noctis_ui::__macro::decode_modal_payload::<Self>(submission)?;
                match submission.get("bio") {
                    Some(noctis_ui::SubmittedValue::TextInput(_)) => Ok(this),
                    _ => Err(noctis_ui::SubmitError::Missing { key: "bio".into() }),
                }
            }
        }
    };
}

pub(crate) use test_route;

test_route!(TBuy, "shop.buy", noctis_codec::RouteShape::INLINE, 0);
test_route!(TSigned, "sig", noctis_codec::RouteShape::INLINE.signed(), 0);

/// A component handler of the `signed` Route [`TSigned`] named `name`
/// that does nothing.
pub(crate) fn signed_button(name: &'static str) -> crate::ComponentHandler {
    crate::ComponentHandler::__new::<TSigned, _, _>(
        name,
        "sig",
        crate::ComponentKind::Button,
        |_: TSigned, _: crate::ComponentCtx| async { Ok(crate::Responded::new()) },
    )
}

/// A component handler of [`TBuy`] named `name` at `route` that does
/// nothing.
pub(crate) fn button(name: &'static str, route: &'static str) -> crate::ComponentHandler {
    crate::ComponentHandler::__new::<TBuy, _, _>(
        name,
        route,
        crate::ComponentKind::Button,
        |_: TBuy, _: crate::ComponentCtx| async { Ok(crate::Responded::new()) },
    )
}

/// A modal handler of [`TBuy`] named `name` at `route` that does nothing.
pub(crate) fn modal(name: &'static str, route: &'static str) -> crate::ModalHandler {
    crate::ModalHandler::__new::<TBuy, _, _>(
        name,
        route,
        |_: TBuy, _: crate::ModalSubmitCtx| async { Ok(crate::Responded::new()) },
    )
}
