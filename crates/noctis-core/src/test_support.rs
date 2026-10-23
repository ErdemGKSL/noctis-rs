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

/// The JSON of guild `id` whose `preferred_locale` is `locale`.
fn guild_json(id: u64, locale: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id.to_string(),
        "name": "g",
        "afk_timeout": 300,
        "default_message_notifications": 0,
        "explicit_content_filter": 0,
        "features": [],
        "emojis": [],
        "mfa_level": 0,
        "nsfw_level": 0,
        "owner_id": "1",
        "preferred_locale": locale,
        "premium_progress_bar_enabled": false,
        "premium_tier": 0,
        "roles": [],
        "system_channel_flags": 0,
        "verification_level": 0,
    })
}

/// An available `GUILD_CREATE` of guild `id` with `preferred_locale`.
pub(crate) fn guild_create(id: u64, locale: &str) -> Event {
    let guild = serde_json::from_value(guild_json(id, locale)).expect("a valid guild");
    Event::GuildCreate(Box::new(
        twilight_model::gateway::payload::incoming::GuildCreate::Available(guild),
    ))
}

/// An unavailable `GUILD_CREATE` of guild `id`.
pub(crate) fn guild_create_unavailable(id: u64) -> Event {
    Event::GuildCreate(Box::new(
        twilight_model::gateway::payload::incoming::GuildCreate::Unavailable(
            twilight_model::guild::UnavailableGuild {
                id: Id::new(id),
                unavailable: true,
            },
        ),
    ))
}

/// A `GUILD_UPDATE` of guild `id` with `preferred_locale`.
pub(crate) fn guild_update(id: u64, locale: &str) -> Event {
    let guild = serde_json::from_value(guild_json(id, locale)).expect("a valid guild");
    Event::GuildUpdate(Box::new(
        twilight_model::gateway::payload::incoming::GuildUpdate(guild),
    ))
}

/// A `GUILD_DELETE` of guild `id`.
pub(crate) fn guild_delete(id: u64, unavailable: Option<bool>) -> Event {
    Event::GuildDelete(twilight_model::gateway::payload::incoming::GuildDelete {
        id: Id::new(id),
        unavailable,
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
/// (`item`): what a kind derive implements, with empty `__Guards` impls
/// (`@unguarded` leaves them to the test). A modal submission also needs
/// the text input `bio`.
macro_rules! test_route {
    ($name:ident, $id:literal, $shape:expr, $schema:literal) => {
        $crate::test_support::test_route!(
            $name,
            $id,
            $shape,
            $schema,
            noctis_codec::RouteRuntime::EMPTY
        );
    };
    ($name:ident, $id:literal, $shape:expr, $schema:literal, $runtime:expr) => {
        $crate::test_support::test_route!(@unguarded $name, $id, $shape, $schema, $runtime);
        impl $crate::__Guards<$crate::ComponentCtx> for $name {}
        impl $crate::__Guards<$crate::ModalSubmitCtx> for $name {}
    };
    // Without the `__Guards` impls: the test writes its own.
    (@unguarded $name:ident, $id:literal, $shape:expr, $schema:literal, $runtime:expr) => {
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

        impl noctis_codec::DeclaredRuntime for $name {
            const RUNTIME: noctis_codec::RouteRuntime = $runtime;
        }
        impl noctis_ui::Handled for $name {}

        impl noctis_ui::ModalForm for $name {
            fn from_submission(
                submission: &noctis_ui::ModalSubmission,
            ) -> Result<Self, noctis_ui::SubmitError> {
                let this = noctis_ui::__macro::decode_modal_payload::<Self>(submission)?;
                <Self as noctis_ui::__macro::ModalFromPayload>::from_payload(this, submission)
            }
        }

        impl noctis_ui::__macro::ModalFromPayload for $name {
            fn from_payload(
                payload: Self,
                submission: &noctis_ui::ModalSubmission,
            ) -> Result<Self, noctis_ui::SubmitError> {
                match submission.get("bio") {
                    Some(noctis_ui::SubmittedValue::TextInput(_)) => Ok(payload),
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

/// One call a [`TestRefs`] store received: the op, the key and, for
/// `put_if_absent`, the TTL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefCall {
    pub(crate) op: &'static str,
    pub(crate) key: String,
    pub(crate) ttl: Option<std::time::Duration>,
}

/// A scripted `RefStore`: a map with TTLs on the tokio clock (so a paused
/// clock expires entries), a call log, scripted `put_if_absent` answers and
/// failing ops.
#[derive(Default)]
pub(crate) struct TestRefs {
    entries: Mutex<std::collections::HashMap<String, (bytes::Bytes, Option<tokio::time::Instant>)>>,
    calls: Mutex<Vec<RefCall>>,
    /// Answers of the next `put_if_absent` calls (`None`: a backend
    /// error); empty means the real behavior.
    put_script: Mutex<std::collections::VecDeque<Option<bool>>>,
    /// Ops that fail with a backend error.
    failing: Mutex<Vec<&'static str>>,
    /// `put_if_absent` fails with `InvalidValue` naming its key, as the
    /// memory store does for an oversized record.
    invalid_puts: std::sync::atomic::AtomicBool,
}

impl TestRefs {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The next `put_if_absent` calls answer `answers` (`None`: an error).
    pub(crate) fn script_puts(&self, answers: impl IntoIterator<Item = Option<bool>>) {
        self.put_script.lock().unwrap().extend(answers);
    }

    /// Every later `put_if_absent` fails with `InvalidValue` under its key.
    pub(crate) fn fail_puts_invalid(&self) {
        self.invalid_puts
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Every later `op` fails with a backend error.
    pub(crate) fn fail(&self, op: &'static str) {
        self.failing.lock().unwrap().push(op);
    }

    /// Stores `value` under the full `key`, without TTL.
    pub(crate) fn insert(&self, key: &str, value: &[u8]) {
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_owned(), (bytes::Bytes::copy_from_slice(value), None));
    }

    /// The live value under the full `key`.
    pub(crate) fn value(&self, key: &str) -> Option<bytes::Bytes> {
        let now = tokio::time::Instant::now();
        self.entries
            .lock()
            .unwrap()
            .get(key)
            .filter(|(_, deadline)| deadline.is_none_or(|d| d > now))
            .map(|(v, _)| v.clone())
    }

    /// The calls so far.
    pub(crate) fn calls(&self) -> Vec<RefCall> {
        self.calls.lock().unwrap().clone()
    }

    /// The calls of `op` so far.
    pub(crate) fn calls_of(&self, op: &str) -> Vec<RefCall> {
        self.calls().into_iter().filter(|c| c.op == op).collect()
    }

    fn record(
        &self,
        op: &'static str,
        key: &str,
        ttl: Option<std::time::Duration>,
    ) -> Result<(), noctis_ports::PortError> {
        self.calls.lock().unwrap().push(RefCall {
            op,
            key: key.to_owned(),
            ttl,
        });
        if self.failing.lock().unwrap().contains(&op) {
            return Err(noctis_ports::PortError::backend("scripted failure"));
        }
        Ok(())
    }

    fn live(&self, key: &str, take: bool) -> Option<bytes::Bytes> {
        let now = tokio::time::Instant::now();
        let mut entries = self.entries.lock().unwrap();
        let live = entries
            .get(key)
            .is_some_and(|(_, deadline)| deadline.is_none_or(|d| d > now));
        if !live {
            entries.remove(key);
            return None;
        }
        if take {
            entries.remove(key).map(|(v, _)| v)
        } else {
            entries.get(key).map(|(v, _)| v.clone())
        }
    }
}

type RefFuture<'a, T> = noctis_ports::BoxFuture<'a, Result<T, noctis_ports::PortError>>;

impl noctis_ports::RefStore for TestRefs {
    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: bytes::Bytes,
        ttl: Option<std::time::Duration>,
    ) -> RefFuture<'a, bool> {
        Box::pin(async move {
            self.record("put_if_absent", key, ttl)?;
            if self.invalid_puts.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(noctis_ports::PortError::invalid_value(
                    key,
                    "record too large",
                ));
            }
            if let Some(answer) = self.put_script.lock().unwrap().pop_front() {
                return answer.ok_or_else(|| noctis_ports::PortError::backend("scripted failure"));
            }
            if self.live(key, false).is_some() {
                return Ok(false);
            }
            let deadline = ttl.map(|t| tokio::time::Instant::now() + t);
            self.entries
                .lock()
                .unwrap()
                .insert(key.to_owned(), (value, deadline));
            Ok(true)
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> RefFuture<'a, Option<bytes::Bytes>> {
        Box::pin(async move {
            self.record("get", key, None)?;
            Ok(self.live(key, false))
        })
    }

    fn take<'a>(&'a self, key: &'a str) -> RefFuture<'a, Option<bytes::Bytes>> {
        Box::pin(async move {
            self.record("take", key, None)?;
            Ok(self.live(key, true))
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> RefFuture<'a, ()> {
        Box::pin(async move {
            self.record("delete", key, None)?;
            self.entries.lock().unwrap().remove(key);
            Ok(())
        })
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: std::time::Duration) -> RefFuture<'a, bool> {
        Box::pin(async move {
            self.record("touch", key, Some(ttl))?;
            Ok(false)
        })
    }
}
