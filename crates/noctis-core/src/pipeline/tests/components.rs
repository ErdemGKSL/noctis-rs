//! Story 7.1 Matrix 12-25 and 27: the component pipeline over a recording
//! upstream. Every rejection is one `warn` with its `reason`, no handler
//! run and no request.

use std::fmt;

use noctis_codec::{CodecError, FieldReader, FieldWriter, Payload, PayloadDecode, RouteShape};

use super::*;
use crate::pipeline::component::run_component;
use crate::test_support::test_route;
use crate::{ComponentCtx, ComponentHandler, ComponentKind, ModalHandler, ModalSubmitCtx};

test_route!(Buy, "shop.buy", RouteShape::INLINE, 0);
test_route!(Pick, "pick", RouteShape::INLINE.select(), 0);
test_route!(ByRef, "r", RouteShape::BY_REF, 0);
test_route!(Signed, "sig", RouteShape::INLINE.signed(), 0);
test_route!(Long, "abcdefghijklmnopqrstuvwx", RouteShape::INLINE, 0);
test_route!(Fails, "fails", RouteShape::INLINE, 0);
test_route!(UserErr, "user.err", RouteShape::INLINE, 0);
test_route!(Panics, "panics", RouteShape::INLINE, 0);
test_route!(Waits, "waits", RouteShape::INLINE, 0);
test_route!(Bio, "bio", RouteShape::INLINE, 0);

/// Nine `u64` fields: the widest test Payload (a 100-character id).
struct Wide([u64; 9]);

impl Payload for Wide {
    const MAX_ENCODED_LEN: Option<usize> = Some(107);

    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|v| out.field(v))
    }
}

impl PayloadDecode<'_> for Wide {
    fn decode_fields(f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
        let mut values = [0; 9];
        for v in &mut values {
            *v = f.field()?;
        }
        Ok(Self(values))
    }
}

impl noctis_codec::Route for Wide {
    const ID: noctis_codec::RouteId = noctis_codec::RouteId::new("w");
    const SHAPE: RouteShape = RouteShape::INLINE;
    const SCHEMA: u8 = 0;
    type Payload = Self;
}

/// A component handler of `S` logging `name:item`.
fn logging<S>(log: &Log, name: &'static str, kind: ComponentKind) -> ComponentHandler
where
    S: noctis_codec::Route<Payload = S> + for<'de> PayloadDecode<'de> + Send + 'static + HasItem,
{
    let log = log.clone();
    ComponentHandler::__new::<S, _, _>(
        name,
        <S as noctis_codec::Route>::ID.as_str(),
        kind,
        move |value: S, ctx: ComponentCtx| {
            log.push(format!(
                "{name}:{}:{}:{}",
                value.item(),
                ctx.module(),
                ctx.user_id()
            ));
            async { Ok(Responded::new()) }
        },
    )
}

/// The `item` field of a test Route.
trait HasItem {
    fn item(&self) -> u64;
}

macro_rules! has_item {
    ($($t:ty),*) => { $(impl HasItem for $t { fn item(&self) -> u64 { self.item } })* };
}

has_item!(
    Buy, Pick, ByRef, Signed, Long, Fails, UserErr, Panics, Waits
);

impl HasItem for Wide {
    fn item(&self) -> u64 {
        self.0[8]
    }
}

/// The Module every test mounts.
fn module(log: &Log, waiting: &Arc<Notify>) -> Module {
    let started = Arc::clone(waiting);
    let bio_log = log.clone();
    Module::new("shop")
        .__add(logging::<Buy>(log, "m::Buy", ComponentKind::Button))
        .__add(logging::<Pick>(log, "m::Pick", ComponentKind::StringSelect))
        .__add(logging::<ByRef>(log, "m::ByRef", ComponentKind::Button))
        .__add(logging::<Signed>(log, "m::Signed", ComponentKind::Button))
        .__add(logging::<Long>(log, "m::Long", ComponentKind::Button))
        .__add(logging::<Wide>(log, "m::Wide", ComponentKind::Button))
        .__add(ComponentHandler::__new::<Fails, _, _>(
            "m::Fails",
            "fails",
            ComponentKind::Button,
            |_: Fails, _: ComponentCtx| async {
                Err(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .__add(ComponentHandler::__new::<UserErr, _, _>(
            "m::UserErr",
            "user.err",
            ComponentKind::Button,
            |_: UserErr, _: ComponentCtx| async {
                Err(HandlerError::user(MessageKey::new("shop.sold_out")))
            },
        ))
        .__add(ComponentHandler::__new::<Panics, _, _>(
            "m::Panics",
            "panics",
            ComponentKind::Button,
            |_: Panics, _: ComponentCtx| async { Ok(explode()) },
        ))
        .__add(ComponentHandler::__new::<Waits, _, _>(
            "m::Waits",
            "waits",
            ComponentKind::Button,
            move |_: Waits, _: ComponentCtx| {
                let started = Arc::clone(&started);
                async move {
                    started.notify_one();
                    std::future::pending::<()>().await;
                    Ok(Responded::new())
                }
            },
        ))
        .__add(ModalHandler::__new::<Bio, _, _>(
            "m::Bio",
            "bio",
            move |bio: Bio, ctx: ModalSubmitCtx| {
                bio_log.push(format!("m::Bio:{}:{}", bio.item, ctx.module()));
                async { Ok(Responded::new()) }
            },
        ))
}

/// Panics: the body of the panicking handler.
fn explode() -> Responded {
    panic!("boom")
}

/// A running App with the test Module and a recording Component sink.
struct Components {
    p: Pipeline,
    sink: Arc<Recording>,
    log: Log,
    waiting: Arc<Notify>,
}

impl Components {
    async fn start() -> Self {
        let log = Log::default();
        let waiting = Arc::new(Notify::new());
        let sink = Arc::new(Recording::default());
        let component_sink = Arc::clone(&sink);
        let p = Pipeline::start_with(module(&log, &waiting), move |b| {
            b.error_sink(ErrorOrigin::Component, component_sink);
        })
        .await;
        Self {
            p,
            sink,
            log,
            waiting,
        }
    }

    async fn run(&self, inbound: Inbound) {
        self.run_with(inbound, CancellationToken::new()).await;
    }

    async fn run_with(&self, inbound: Inbound, cancel: CancellationToken) {
        run_component(
            Arc::clone(self.p.fx.app.registry()),
            Arc::clone(self.p.fx.app.shared()),
            inbound,
            cancel,
        )
        .await;
    }

    /// Runs `inbound` and asserts one rejection `reason`, no handler run,
    /// no request and no report.
    async fn rejects(&self, inbound: Inbound, reason: &str) {
        let capture = Capture::default();
        let lines = {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            self.run(inbound).await;
            capture.lines_with(&["noctis::dispatch", "component interaction rejected"])
        };
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(
            lines[0].contains(&format!("reason=\"{reason}\"")),
            "{reason}: {}",
            lines[0]
        );
        assert!(self.log.lines().is_empty(), "{:?}", self.log.lines());
        assert!(self.p.requests().is_empty());
        assert!(self.sink.seen().is_empty());
    }
}

/// A `MESSAGE_COMPONENT` interaction (id 100, token `tok`, application 7,
/// user 3) on a component of `component_type` with `custom_id`.
fn click(custom_id: &str, component_type: u8) -> Value {
    json!({
        "id": "100",
        "application_id": "7",
        "type": 3,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user("3"),
        "locale": "tr",
        "data": { "custom_id": custom_id, "component_type": component_type },
    })
}

/// A `MODAL_SUBMIT` interaction with `custom_id` and the text input `bio`
/// when `with_bio`.
fn submit(custom_id: &str, with_bio: bool) -> Value {
    let components = if with_bio {
        json!([{ "type": 18, "id": 1, "component": { "type": 4, "id": 2, "custom_id": "bio", "value": "hi" } }])
    } else {
        json!([])
    };
    json!({
        "id": "100",
        "application_id": "7",
        "type": 5,
        "token": "tok",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
        "user": user("3"),
        "data": { "custom_id": custom_id, "components": components },
    })
}

/// Matrix 1 (unit): a click runs the handler once with the decoded value.
#[tokio::test]
async fn click_runs_the_handler() {
    let c = Components::start().await;
    c.run(inbound(&click("0shop.buy:47", 2))).await;
    assert_eq!(c.log.lines(), ["m::Buy:255:shop:3"]);
    assert!(c.p.requests().is_empty());
    assert!(c.sink.seen().is_empty());
}

/// Matrix 12-16: malformed and unknown ids.
#[tokio::test]
async fn malformed_and_unknown_ids() {
    let c = Components::start().await;
    for id in ["", "0shöp:1", "0Shop.buy:47"] {
        c.rejects(inbound(&click(id, 2)), "malformed").await;
    }
    let ten = "10000000000";
    let exact = format!("0w:{}", [ten; 8].join(",")) + ",1";
    assert_eq!(exact.len(), 100);
    c.run(inbound(&click(&exact, 2))).await;
    assert_eq!(
        c.log.0.lock().unwrap().drain(..).collect::<Vec<_>>(),
        ["m::Wide:1:shop:3"]
    );
    c.rejects(inbound(&click(&format!("{exact}0"), 2)), "malformed")
        .await;
    c.run(inbound(&click("0abcdefghijklmnopqrstuvwx:1", 2)))
        .await;
    assert_eq!(
        c.log.0.lock().unwrap().drain(..).collect::<Vec<_>>(),
        ["m::Long:1:shop:3"]
    );
    c.rejects(
        inbound(&click("0abcdefghijklmnopqrstuvw:1", 2)),
        "unknown_route",
    )
    .await;
    c.rejects(inbound(&click("0nope:", 2)), "unknown_route")
        .await;
}

/// Matrix 17-19: family and kind checks; a select with its nonce.
#[tokio::test]
async fn family_and_kind() {
    let c = Components::start().await;
    c.rejects(inbound(&click("0bio:1", 2)), "unknown_route")
        .await;
    let modal = submit("0shop.buy:47", true);
    c.rejects(inbound(&modal), "unknown_route").await;
    c.rejects(inbound(&click("0shop.buy:47", 3)), "kind_mismatch")
        .await;
    c.run(inbound(&click("4pick:0147", 3))).await;
    assert_eq!(c.log.lines(), ["m::Pick:255:shop:3"]);
}

/// Matrix 20-23: stale, Ref, signed and undecodable bodies.
#[tokio::test]
async fn rejected_bodies() {
    let c = Components::start().await;
    c.rejects(inbound(&click("1shop.buy:147", 2)), "stale")
        .await;
    c.rejects(inbound(&click("0r:~0123456789A", 2)), "ref_unresolved")
        .await;
    c.rejects(inbound(&click("8sig:47abcdef", 2)), "forged")
        .await;
    c.rejects(inbound(&click("0sig:47", 2)), "forged").await;
    c.rejects(inbound(&click("0shop.buy:", 2)), "undecodable")
        .await;
    c.rejects(inbound(&click("0shop.buy:47,9", 2)), "undecodable")
        .await;
}

/// Matrix 24: no author; no data.
#[tokio::test]
async fn no_author_or_data() {
    let c = Components::start().await;
    let mut anonymous = click("0shop.buy:47", 2);
    anonymous.as_object_mut().unwrap().remove("user");
    c.rejects(inbound(&anonymous), "no_author").await;
    // Twilight requires `data` on a click, so it is removed after parsing.
    let mut no_data = inbound(&click("0shop.buy:47", 2));
    if let InboundKind::Interaction(interaction, _) = &mut no_data.kind {
        interaction.data = None;
    }
    c.rejects(no_data, "malformed").await;
}

/// Matrix 25: an error, a user condition, a panic (the next click is
/// served) and a cancellation.
#[tokio::test]
async fn handler_failures() {
    use metrics_util::MetricKind;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let c = Components::start().await;
    c.run(inbound(&click("0fails:1", 2))).await;
    let seen = c.sink.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (
            seen[0].origin,
            seen[0].kind.as_str(),
            seen[0].module.as_deref(),
            seen[0].handler.as_deref(),
            seen[0].event_type.as_deref(),
            seen[0].shard,
        ),
        (
            ErrorOrigin::Component,
            "error:boom",
            Some("shop"),
            Some("m::Fails"),
            Some("INTERACTION_CREATE"),
            Some(0),
        )
    );
    let snap = snapshotter.snapshot().into_vec();
    let errors: Vec<_> = snap
        .iter()
        .filter(|(k, ..)| {
            k.kind() == MetricKind::Counter && k.key().name() == "noctis_handler_errors_total"
        })
        .collect();
    assert_eq!(errors.len(), 1, "{snap:?}");
    assert!(matches!(errors[0].3, DebugValue::Counter(1)), "{snap:?}");
    assert!(
        errors[0]
            .0
            .key()
            .labels()
            .any(|l| l.key() == "kind" && l.value() == "component")
    );
    assert!(snap.iter().any(|(k, ..)| {
        k.key().name() == "noctis_handler_duration_seconds"
            && k.key().labels().any(|l| l.value() == "component")
    }));

    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        c.run(inbound(&click("0user.err:1", 2))).await;
    }
    assert_eq!(c.sink.seen().len(), 1);
    assert_eq!(
        capture
            .lines_with(&[
                "DEBUG",
                "noctis::component",
                "user condition",
                "shop.sold_out"
            ])
            .len(),
        1,
        "{}",
        capture.text()
    );

    c.run(inbound(&click("0panics:1", 2))).await;
    assert_eq!(c.sink.seen()[1].kind, "panic:boom");
    c.run(inbound(&click("0shop.buy:1", 2))).await;
    assert_eq!(c.log.lines(), ["m::Buy:1:shop:3"]);

    let cancel = CancellationToken::new();
    let run = c.run_with(inbound(&click("0waits:1", 2)), cancel.clone());
    let trigger = async {
        c.waiting.notified().await;
        cancel.cancel();
    };
    tokio::join!(run, trigger);
    assert_eq!(c.sink.seen()[2].kind, "cancelled");
    assert!(c.p.requests().is_empty());
}

/// Matrix 26 and 27: a modal submit runs its handler; unparsable or
/// invalid submissions are rejected.
#[tokio::test]
async fn modal_submits() {
    let c = Components::start().await;
    c.run(inbound(&submit("0bio:47", true))).await;
    assert_eq!(
        c.log.0.lock().unwrap().drain(..).collect::<Vec<_>>(),
        ["m::Bio:255:shop"]
    );
    let parsed = submit("0bio:47", true);
    c.rejects(inbound_raw(&parsed, &json!("not json")), "malformed")
        .await;
    let mut no_id = parsed.clone();
    no_id["data"].as_object_mut().unwrap().remove("custom_id");
    c.rejects(inbound_raw(&parsed, &no_id), "malformed").await;
    c.rejects(inbound(&submit("0bio:47", false)), "submit_invalid")
        .await;
    c.rejects(inbound(&submit("0bio:", true)), "undecodable")
        .await;
}
