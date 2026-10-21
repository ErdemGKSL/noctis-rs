//! Story 7.1 Matrix 12-25 and 27 and Story 7.2 Matrix 1-12, 29-31, 34-38,
//! 40-51 and S1-S4, S17, S18: the component pipeline over a recording upstream. Every
//! rejection is one `warn` with its `reason`, no handler run, and the
//! generic ephemeral notice. Stale and Ref ids go to the Expired Handler
//! chain (`super::expired`, Story 7.3).

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
test_route!(SyncPanics, "sync.panics", RouteShape::INLINE, 0);

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

impl noctis_codec::DeclaredRuntime for Wide {}
impl crate::__Guards<ComponentCtx> for Wide {}

impl noctis_codec::Route for Wide {
    const ID: noctis_codec::RouteId = noctis_codec::RouteId::new("w");
    const SHAPE: RouteShape = RouteShape::INLINE;
    const SCHEMA: u8 = 0;
    type Payload = Self;
}

/// A component handler of `S` logging `name:item`.
fn logging<S>(log: &Log, name: &'static str, kind: ComponentKind) -> ComponentHandler
where
    S: noctis_codec::Route<Payload = S>
        + noctis_codec::DeclaredRuntime
        + crate::__Guards<ComponentCtx>
        + for<'de> PayloadDecode<'de>
        + Send
        + 'static
        + HasItem,
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
        .__add(ComponentHandler::__new::<SyncPanics, _, _>(
            "m::SyncPanics",
            "sync.panics",
            ComponentKind::Button,
            |_: SyncPanics, _: ComponentCtx| -> std::future::Ready<HandlerResult<Responded>> {
                panic!("sync")
            },
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

/// The current payload key of the test App.
fn key_a() -> noctis_codec::PayloadKey {
    noctis_codec::PayloadKey::from_bytes([0xab; 32])
}

/// Another payload key.
fn key_b() -> noctis_codec::PayloadKey {
    noctis_codec::PayloadKey::from_bytes([0xcd; 32])
}

impl Components {
    async fn start() -> Self {
        Self::start_with(discord(), |b| {
            b.payload_key(key_a());
        })
        .await
    }

    /// As [`Components::start`], answering `answer`, with `configure`
    /// applied to the builder (which needs a payload key: a `signed` Route
    /// is mounted).
    async fn start_with(answer: Answer, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let log = Log::default();
        let waiting = Arc::new(Notify::new());
        let sink = Arc::new(Recording::default());
        let component_sink = Arc::clone(&sink);
        let p = Pipeline::start_answering(module(&log, &waiting), answer, move |b| {
            b.error_sink(ErrorOrigin::Component, component_sink);
            configure(b);
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
        let requests = self.p.requests();
        assert_eq!(requests.len(), 1, "{reason}: {requests:?}");
        assert_generic_notice(&requests[0]);
        self.clear();
        assert!(self.sink.seen().is_empty());
    }

    /// Forgets the recorded requests.
    fn clear(&self) {
        self.p.fx.server.log.lock().unwrap().clear();
    }
}

/// Asserts `request` is the CID-06 notice: one ephemeral type-4 callback
/// holding one Text Display, without a bot token.
pub(super) fn assert_generic_notice(request: &(Method, String, Value)) {
    assert_eq!(
        (&request.0, request.1.as_str()),
        (&Method::POST, CB),
        "{request:?}"
    );
    assert_eq!(request.2["type"], 4, "{request:?}");
    assert_eq!(request.2["data"]["flags"], 32832, "{request:?}");
    assert_eq!(
        request.2["data"]["allowed_mentions"],
        json!({ "parse": [] })
    );
    let components = request.2["data"]["components"].as_array().unwrap();
    assert_eq!(components.len(), 1, "{request:?}");
    assert_eq!(components[0]["type"], 10, "{request:?}");
    assert!(
        components[0]["content"]
            .as_str()
            .is_some_and(|t| !t.is_empty())
    );
}

/// A `MESSAGE_COMPONENT` interaction (id 100, token `tok`, application 7,
/// user 3) on a component of `component_type` with `custom_id`.
pub(super) fn click(custom_id: &str, component_type: u8) -> Value {
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
pub(super) fn submit(custom_id: &str, with_bio: bool) -> Value {
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

/// Matrix 20-23: signed and undecodable bodies (stale and Ref ids: Story
/// 7.3, `super::expired`).
#[tokio::test]
async fn cid_06_rejections_send_the_notice() {
    let c = Components::start().await;
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
    assert_eq!(
        c.log.0.lock().unwrap().drain(..).collect::<Vec<_>>(),
        ["m::Buy:1:shop:3"]
    );

    // A panic before the handler returns its future (the start-time guard).
    c.run(inbound(&click("0sync.panics:1", 2))).await;
    let seen = c.sink.seen();
    assert_eq!(
        (
            seen[2].kind.as_str(),
            seen[2].module.as_deref(),
            seen[2].handler.as_deref()
        ),
        ("panic:sync", Some("shop"), Some("m::SyncPanics"))
    );
    c.run(inbound(&click("0shop.buy:1", 2))).await;
    assert_eq!(c.log.lines(), ["m::Buy:1:shop:3"]);

    let cancel = CancellationToken::new();
    let run = c.run_with(inbound(&click("0waits:1", 2)), cancel.clone());
    let trigger = async {
        c.waiting.notified().await;
        cancel.cancel();
    };
    tokio::join!(run, trigger);
    assert_eq!(c.sink.seen()[3].kind, "cancelled");
    // One ephemeral notice per failure before any ack; none for the
    // cancellation (Story 7.2 Matrix 49-51).
    let requests = c.p.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    for request in &requests {
        assert_generic_notice(request);
    }
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
    let mut twice = submit("0bio:47", true);
    let input = twice["data"]["components"][0].clone();
    twice["data"]["components"]
        .as_array_mut()
        .unwrap()
        .push(input);
    c.rejects(inbound(&twice), "submit_invalid").await;
}

// ---- Story 7.2: responses, notices and keys ----

mod responses {
    use std::future::Future;

    use noctis_codec::{Parts, encode};
    use noctis_i18n::Builtin;
    use noctis_ui::{Modal, ModalLabel, TextInput, TextInputStyle};

    use super::*;
    use crate::kind::__Response;
    use crate::{AutoDefer, ComponentResponder, Manual, ModalResponder};

    test_route!(Upd, "upd", RouteShape::INLINE, 0);
    test_route!(Eph, "eph", RouteShape::INLINE, 0);
    test_route!(Pubr, "pubr", RouteShape::INLINE, 0);
    test_route!(Ret, "ret", RouteShape::INLINE, 0);
    test_route!(Silent, "silent", RouteShape::INLINE, 0);
    test_route!(Reply6, "reply6", RouteShape::INLINE, 0);
    test_route!(Open, "open", RouteShape::INLINE, 0);
    test_route!(ManEph, "man.eph", RouteShape::INLINE, 0);
    test_route!(Breaks, "breaks", RouteShape::INLINE, 0);
    test_route!(BioUp, "bio.up", RouteShape::INLINE, 0);
    test_route!(BioRet, "bio.ret", RouteShape::INLINE, 0);
    test_route!(BioEph, "bio.eph", RouteShape::INLINE, 0);
    test_route!(BioPub, "bio.pub", RouteShape::INLINE, 0);
    test_route!(BioMan, "bio.man", RouteShape::INLINE, 0);

    /// The ack deadline of these tests.
    const D: Duration = Duration::from_millis(50);

    fn sleep_ms(ms: u64) -> tokio::time::Sleep {
        tokio::time::sleep(Duration::from_millis(ms))
    }

    /// A component handler of `S` declared `response`, taking a responder
    /// of mode `D`.
    fn handler<S, M, F, Fut, R>(name: &'static str, response: __Response, f: F) -> ComponentHandler
    where
        S: noctis_codec::Route<Payload = S>
            + noctis_codec::DeclaredRuntime
            + crate::__Guards<ComponentCtx>
            + for<'de> PayloadDecode<'de>
            + Send
            + 'static,
        M: crate::DeferMode,
        F: Fn(S, ComponentCtx, crate::Responder<crate::Component, crate::Pending, M>) -> Fut
            + Send
            + Sync
            + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: crate::IntoResponse<crate::Component>,
    {
        ComponentHandler::__new_with_responder::<S, M, _, _, _>(
            name,
            <S as noctis_codec::Route>::ID.as_str(),
            ComponentKind::Button,
            response,
            f,
        )
    }

    /// The modal of `open`.
    fn form() -> Modal {
        noctis_ui::__macro::build_modal::<BioUp>(BioUp { item: 1 }, "Bio").push(ModalLabel::new(
            "Bio",
            TextInput::new("bio", TextInputStyle::Short),
        ))
    }

    /// Every Story 7.2 handler; `item` selects the behaviour.
    #[expect(
        clippy::too_many_lines,
        reason = "one handler per Story 7.2 matrix row"
    )]
    fn module() -> Module {
        Module::new("ui")
            .__add(handler::<Upd, AutoDefer, _, _, _>(
                "ui::Upd",
                __Response::Update,
                |v: Upd, _, r: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(r.update(v2("u")).await?.done())
                },
            ))
            .__add(handler::<Eph, AutoDefer, _, _, _>(
                "ui::Eph",
                __Response::Ephemeral,
                |v: Eph, _, _: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(handler::<Pubr, AutoDefer, _, _, _>(
                "ui::Pubr",
                __Response::Public,
                |v: Pubr, _, _: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(handler::<Ret, AutoDefer, _, _, _>(
                "ui::Ret",
                __Response::Update,
                |v: Ret, _, _: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(handler::<Silent, AutoDefer, _, _, _>(
                "ui::Silent",
                __Response::Update,
                |v: Silent, _, _: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(Responded::new())
                },
            ))
            .__add(handler::<Reply6, AutoDefer, _, _, _>(
                "ui::Reply6",
                __Response::Update,
                |v: Reply6, _, r: ComponentResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(r.reply(v2("r")).await?.done())
                },
            ))
            .__add(handler::<Open, Manual, _, _, _>(
                "ui::Open",
                __Response::Update,
                |v: Open, _, r: ComponentResponder<Manual>| async move {
                    match v.item {
                        0 => {
                            sleep_ms(100).await;
                            Ok(r.show_modal(form()).await?.done())
                        }
                        1 => {
                            sleep_ms(100).await;
                            drop(r);
                            Ok(Responded::new())
                        }
                        2 => Ok(r.defer_update().await?.done()),
                        _ => Ok(r.defer_reply().await?.done()),
                    }
                },
            ))
            .__add(handler::<ManEph, Manual, _, _, _>(
                "ui::ManEph",
                __Response::Ephemeral,
                |_: ManEph, _, r: ComponentResponder<Manual>| async move {
                    Ok(r.reply(v2("e")).await?.done())
                },
            ))
            .__add(handler::<Breaks, AutoDefer, _, _, _>(
                "ui::Breaks",
                __Response::Update,
                |v: Breaks, _, _: ComponentResponder| async move {
                    if v.item == 0 {
                        return Err::<Responded, _>(HandlerError::new(std::io::Error::other(
                            "broken",
                        )));
                    }
                    sleep_ms(v.item).await;
                    panic!("late")
                },
            ))
            .__add(ModalHandler::__new_with_responder::<
                BioUp,
                AutoDefer,
                _,
                _,
                _,
            >(
                "ui::BioUp",
                "bio.up",
                __Response::Update,
                |_: BioUp, _: ModalSubmitCtx, r: ModalResponder| async move {
                    Ok(r.update(v2("u")).await?.done())
                },
            ))
            .__add(ModalHandler::__new_with_responder::<
                BioRet,
                AutoDefer,
                _,
                _,
                _,
            >(
                "ui::BioRet",
                "bio.ret",
                __Response::Update,
                |v: BioRet, _: ModalSubmitCtx, _: ModalResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(ModalHandler::__new_with_responder::<
                BioEph,
                AutoDefer,
                _,
                _,
                _,
            >(
                "ui::BioEph",
                "bio.eph",
                __Response::Ephemeral,
                |v: BioEph, _: ModalSubmitCtx, _: ModalResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(ModalHandler::__new_with_responder::<
                BioPub,
                AutoDefer,
                _,
                _,
                _,
            >(
                "ui::BioPub",
                "bio.pub",
                __Response::Public,
                |v: BioPub, _: ModalSubmitCtx, _: ModalResponder| async move {
                    sleep_ms(v.item).await;
                    Ok(v2("m"))
                },
            ))
            .__add(
                ModalHandler::__new_with_responder::<BioMan, Manual, _, _, _>(
                    "ui::BioMan",
                    "bio.man",
                    __Response::Update,
                    |v: BioMan, _: ModalSubmitCtx, r: ModalResponder<Manual>| async move {
                        match v.item {
                            0 => Ok(r.reply(v2("r")).await?.done()),
                            1 => {
                                sleep_ms(100).await;
                                drop(r);
                                Ok(Responded::new())
                            }
                            _ => {
                                let acked = r.defer_reply().await?;
                                acked.edit_reply(v2("m")).await?;
                                Ok(acked.done())
                            }
                        }
                    },
                ),
            )
    }

    /// A running App with [`module`], `D` as the ack deadline, a recording
    /// Component sink and the payload key ring `configure` sets.
    struct Ui {
        p: Pipeline,
        sink: Arc<Recording>,
    }

    impl Ui {
        async fn start() -> Self {
            Self::start_answering(discord()).await
        }

        async fn start_answering(answer: Answer) -> Self {
            let sink = Arc::new(Recording::default());
            let component_sink = Arc::clone(&sink);
            let p = Pipeline::start_answering(module(), answer, move |b| {
                b.error_sink(ErrorOrigin::Component, component_sink)
                    .ack_deadline(D);
            })
            .await;
            Self { p, sink }
        }

        async fn run(&self, inbound: Inbound) {
            run_component(
                Arc::clone(self.p.fx.app.registry()),
                Arc::clone(self.p.fx.app.shared()),
                inbound,
                CancellationToken::new(),
            )
            .await;
        }

        /// The requests so far, then forgets them.
        fn take(&self) -> Vec<(Method, String, Value)> {
            let requests = self.p.requests();
            self.p.fx.server.log.lock().unwrap().clear();
            requests
        }
    }

    /// The raw JSON of `parsed` with the source message 77 in channel 55
    /// (`flags`), which twilight does not need to parse.
    fn on_message(parsed: &Value, flags: u64) -> Inbound {
        let mut raw = parsed.clone();
        raw["message"] =
            json!({ "id": "77", "channel_id": "55", "flags": flags, "components": [] });
        inbound_raw(parsed, &raw)
    }

    /// A click on `route` with `item` on a V2 message.
    fn press(route: &str, item: u64) -> Inbound {
        on_message(&click(&format!("0{route}:{}", base62(item)), 2), 32768)
    }

    /// `n` in base62 (the inline `u64` encoding).
    fn base62(n: u64) -> String {
        let mut out = String::new();
        encode_u64(n, &mut out);
        out
    }

    fn encode_u64(n: u64, out: &mut String) {
        use noctis_codec::Field;
        n.encode(out).unwrap();
    }

    fn update_data(text: &str) -> Value {
        json!({
            "flags": 32768,
            "components": [{ "type": 10, "id": 1, "content": text }],
            "allowed_mentions": { "parse": [] },
            "attachments": [],
        })
    }

    /// Matrix 20, 29 and 31: `update` answers with type 7; past the deadline
    /// the auto-defer sent type 6 first and the update is an edit.
    #[tokio::test]
    async fn update_and_its_auto_defer() {
        let metrics = Metrics::new();
        let _metrics = metrics::set_default_local_recorder(&metrics.recorder);
        let ui = Ui::start().await;
        ui.run(press("upd", 0)).await;
        assert_eq!(
            ui.take(),
            [(
                Method::POST,
                CB.to_owned(),
                json!({ "type": 7, "data": update_data("u") })
            )]
        );
        ui.run(press("upd", 150)).await;
        assert_eq!(
            ui.take(),
            [
                (Method::POST, CB.to_owned(), json!({ "type": 6 })),
                (Method::PATCH, ORIGINAL.to_owned(), update_data("u")),
            ]
        );
        assert_eq!(metrics.read().0, 1);
        assert!(ui.sink.seen().is_empty());
    }

    /// Matrix 30, 32 and 34: the declared response shapes the auto-defer
    /// and a returned message.
    #[tokio::test]
    async fn declared_responses() {
        let ui = Ui::start().await;
        ui.run(press("eph", 150)).await;
        let requests = ui.take();
        assert_eq!(requests[0].2, json!({ "type": 5, "data": { "flags": 64 } }));
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::PATCH, ORIGINAL)
        );
        assert_eq!(requests[1].2["flags"], 32768);
        ui.run(press("pubr", 150)).await;
        let requests = ui.take();
        assert_eq!(requests[0].2, json!({ "type": 5 }));
        assert_eq!(requests[1].0, Method::PATCH);

        ui.run(press("ret", 0)).await;
        assert_eq!(
            ui.take()[0].2,
            json!({ "type": 7, "data": update_data("m") })
        );
        ui.run(press("eph", 0)).await;
        let requests = ui.take();
        assert_eq!(
            (requests[0].1.as_str(), &requests[0].2["type"]),
            (CB_REPLY, &json!(4))
        );
        assert_eq!(requests[0].2["data"]["flags"], 32832);
        ui.run(press("silent", 0)).await;
        assert!(ui.take().is_empty());
        ui.run(press("silent", 150)).await;
        assert_eq!(
            ui.take(),
            [(Method::POST, CB.to_owned(), json!({ "type": 6 }))]
        );

        // A `reply` after the type-6 auto-defer is a follow-up.
        ui.run(press("reply6", 150)).await;
        let requests = ui.take();
        assert_eq!(requests[0].2, json!({ "type": 6 }));
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::POST, "/api/v10/webhooks/7/tok?wait=true")
        );
        assert_eq!(requests.len(), 2, "{requests:?}");
        for seen in ui.p.fx.server.seen() {
            assert!(seen.headers.get("authorization").is_none());
        }
    }

    /// Matrix 35-38: `auto_defer = false` components.
    #[tokio::test]
    async fn manual_components() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let ui = Ui::start().await;
        ui.run(press("open", 0)).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(requests[0].2["type"], 9);
        assert_eq!(requests[0].2["data"]["title"], "Bio");
        assert_eq!(requests[0].2["data"]["custom_id"], "0bio.up:1");

        let missed = [
            "noctis::dispatch",
            "did not answer within the 3 s ack deadline",
        ];
        let mut late = press("open", 1);
        late.received_at = Instant::now()
            .checked_sub(Duration::from_millis(2950))
            .unwrap();
        ui.run(late).await;
        assert!(ui.take().is_empty());
        let lines = capture.lines_with(&missed);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(
            lines[0].contains("handler=\"ui::Open\"") && lines[0].contains("module=\"ui\""),
            "{}",
            lines[0]
        );

        ui.run(press("open", 2)).await;
        assert_eq!(
            ui.take(),
            [(Method::POST, CB.to_owned(), json!({ "type": 6 }))]
        );
        ui.run(press("open", 3)).await;
        assert_eq!(
            ui.take(),
            [(Method::POST, CB.to_owned(), json!({ "type": 5 }))]
        );
        ui.run(press("man.eph", 0)).await;
        assert_eq!(ui.take()[0].2["data"]["flags"], 32832);
        assert_eq!(capture.lines_with(&missed).len(), 1);
    }

    /// Matrix 40-43: modal submits with and without a source message.
    #[tokio::test]
    async fn modal_submit_responses() {
        let ui = Ui::start().await;
        let opened = |item: &str| submit(&format!("0bio.up:{item}"), true);
        let returns = |item: u64| submit(&format!("0bio.ret:{}", base62(item)), true);
        ui.run(on_message(&opened("0"), 32768)).await;
        assert_eq!(
            ui.take()[0].2,
            json!({ "type": 7, "data": update_data("u") })
        );
        // From a slash command: `update` is unavailable (the handler error
        // sends the notice).
        ui.run(inbound(&opened("0"))).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_generic_notice(&requests[0]);
        assert!(
            ui.sink.seen()[0].kind.contains("no message to update"),
            "{:?}",
            ui.sink.seen()
        );

        ui.run(on_message(&returns(150), 32768)).await;
        let requests = ui.take();
        assert_eq!(requests[0].2, json!({ "type": 6 }));
        assert_eq!(requests[1].0, Method::PATCH);
        ui.run(inbound(&returns(150))).await;
        let requests = ui.take();
        assert_eq!(requests[0].2, json!({ "type": 5 }));
        assert_eq!(requests[1].0, Method::PATCH);

        ui.run(on_message(&returns(0), 32768)).await;
        assert_eq!(ui.take()[0].2["type"], 7);
        ui.run(inbound(&returns(0))).await;
        let requests = ui.take();
        assert_eq!(
            (requests[0].1.as_str(), &requests[0].2["type"]),
            (CB_REPLY, &json!(4))
        );
        assert_eq!(requests[0].2["data"]["flags"], 32768);
    }

    /// Story 7.8 rows 19-21: a submit's declared `ephemeral` or `public`
    /// shapes the auto-defer (type 5, never type 6, with or without a
    /// source message) and a returned message.
    #[tokio::test]
    async fn modal_declared_responses() {
        let ui = Ui::start().await;
        let at = |route: &str, item: u64| submit(&format!("0{route}:{}", base62(item)), true);
        for from_message in [true, false] {
            let wrap = |json: &Value| {
                if from_message {
                    on_message(json, 32768)
                } else {
                    inbound(json)
                }
            };
            // Row 19: slow `ephemeral` defers with EPHEMERAL, then edits.
            ui.run(wrap(&at("bio.eph", 150))).await;
            let requests = ui.take();
            assert_eq!(requests.len(), 2, "{requests:?}");
            assert_eq!(requests[0].2, json!({ "type": 5, "data": { "flags": 64 } }));
            assert_eq!(
                (&requests[1].0, requests[1].1.as_str()),
                (&Method::PATCH, ORIGINAL)
            );
            assert_eq!(requests[1].2["flags"], 32768);
            // Row 20: fast `ephemeral` is one ephemeral type-4 reply.
            ui.run(wrap(&at("bio.eph", 0))).await;
            let requests = ui.take();
            assert_eq!(requests.len(), 1, "{requests:?}");
            assert_eq!(
                (requests[0].1.as_str(), &requests[0].2["type"]),
                (CB_REPLY, &json!(4))
            );
            assert_eq!(requests[0].2["data"]["flags"], 32832);
            // Row 21: slow `public` is type 5 without EPHEMERAL.
            ui.run(wrap(&at("bio.pub", 150))).await;
            let requests = ui.take();
            assert_eq!(requests[0].2, json!({ "type": 5 }));
            assert_eq!(requests[1].0, Method::PATCH);
            assert_eq!(requests[1].2["flags"], 32768);
            ui.run(wrap(&at("bio.pub", 0))).await;
            let requests = ui.take();
            assert_eq!(requests[0].2["type"], 4);
            assert_eq!(requests[0].2["data"]["flags"], 32768);
        }
        assert!(ui.sink.seen().is_empty(), "{:?}", ui.sink.seen());
    }

    /// Story 7.8 rows 22-24: an `auto_defer = false` submit is never
    /// auto-deferred.
    #[tokio::test]
    async fn manual_modal_submits() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let ui = Ui::start().await;
        let at =
            |item: u64| on_message(&submit(&format!("0bio.man:{}", base62(item)), true), 32768);
        // Row 22: it answers itself, one type-4 callback.
        ui.run(at(0)).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(
            (requests[0].1.as_str(), &requests[0].2["type"]),
            (CB_REPLY, &json!(4))
        );
        // Row 23: silent past the 3 s deadline: no request, one error log.
        let missed = [
            "noctis::dispatch",
            "did not answer within the 3 s ack deadline",
        ];
        let mut late = at(1);
        late.received_at = Instant::now()
            .checked_sub(Duration::from_millis(2950))
            .unwrap();
        ui.run(late).await;
        assert!(ui.take().is_empty());
        let lines = capture.lines_with(&missed);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("handler=\"ui::BioMan\""), "{}", lines[0]);
        // Row 24: it defers itself, then edits the original.
        ui.run(at(2)).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(
            (&requests[0].0, requests[0].1.as_str(), &requests[0].2),
            (&Method::POST, CB, &json!({ "type": 5 }))
        );
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::PATCH, ORIGINAL)
        );
    }

    /// Matrix 49: an error before any ack is the type-4 notice; a panic
    /// after the type-6 auto-defer is an ephemeral follow-up, and the
    /// clicked message is not deleted.
    #[tokio::test]
    async fn failures_after_an_update_defer() {
        let ui = Ui::start().await;
        ui.run(press("breaks", 0)).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 1);
        assert_generic_notice(&requests[0]);
        ui.run(press("breaks", 150)).await;
        let requests = ui.take();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(requests[0].2, json!({ "type": 6 }));
        assert_eq!(
            (&requests[1].0, requests[1].1.as_str()),
            (&Method::POST, "/api/v10/webhooks/7/tok?wait=true")
        );
        assert_eq!(requests[1].2["flags"], 32832);
        let kinds: Vec<_> = ui.sink.seen().into_iter().map(|s| s.kind).collect();
        assert_eq!(kinds, ["error:broken", "panic:late"]);
    }

    /// Matrix 7: the notice is rendered in the User, then the Guild Locale,
    /// else en-US (`Builtin` falls back to en-US text).
    #[tokio::test]
    async fn notice_locale() {
        let ui = Ui::start().await;
        let text = |locale: Locale| notice::text(&Builtin, locale, keys::ERROR_GENERIC);
        let cases = [
            (json!({ "locale": "tr" }), text("tr".parse().unwrap())),
            (
                json!({ "guild_locale": "de", "guild_id": "9" }),
                text("de".parse().unwrap()),
            ),
            (json!({}), GENERIC.to_owned()),
            (json!({ "locale": "xx-YY" }), GENERIC.to_owned()),
        ];
        for (extra, want) in cases {
            let mut d = click("0nope:", 2);
            d.as_object_mut().unwrap().remove("locale");
            d.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            ui.run(inbound(&d)).await;
            let requests = ui.take();
            assert_eq!(requests.len(), 1, "{extra}");
            assert_eq!(
                requests[0].2["data"]["components"][0]["content"], want,
                "{extra}"
            );
        }
        // `Builtin` ships en-US only, so every locale renders its text.
    }

    /// Matrix 8: a failed notice is one warning.
    #[tokio::test]
    async fn notice_send_failure_is_a_warning() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let answer: Answer = Arc::new(|_: &crate::edge::test_support::Seen| {
            (
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                bytes::Bytes::new(),
            )
        });
        let ui = Ui::start_answering(answer).await;
        ui.run(inbound(&click("0nope:", 2))).await;
        let lines = capture.lines_with(&["noctis::dispatch", "could not send the error notice"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("WARN"), "{}", lines[0]);
    }

    /// The id of the signed test Route `sig` with `item`, tagged with `key`.
    fn signed_id(item: u64, key: &noctis_codec::PayloadKey) -> String {
        encode::<Signed>(&Signed { item }, &Parts::new().with_signer(key))
            .unwrap()
            .to_string()
    }

    /// Matrix 9-12 and 16: signed ids verify with the current or the
    /// previous key; a bad or missing tag is `forged`; no log line or span
    /// field shows the key.
    #[tokio::test]
    async fn cid_06_signed_routes_and_rotation() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let c = Components::start_with(discord(), |b| {
            b.payload_key(key_a()).previous_payload_key(key_b());
        })
        .await;
        c.run(inbound(&click(&signed_id(47, &key_a()), 2))).await;
        c.run(inbound(&click(&signed_id(48, &key_b()), 2))).await;
        assert_eq!(
            c.log.lines(),
            ["m::Signed:47:shop:3", "m::Signed:48:shop:3"]
        );
        let mut flipped = signed_id(47, &key_a());
        let last = flipped.pop().unwrap();
        flipped.push(if last == 'a' { 'b' } else { 'a' });
        c.log.0.lock().unwrap().clear();
        c.rejects(inbound(&click(&flipped, 2)), "forged").await;
        // The tag and the `T` flag stripped: a signed Route's inline id
        // without a tag is `MissingTag`.
        c.rejects(inbound(&click("0sig:47", 2)), "forged").await;
        let text = capture.text();
        assert!(!text.contains("abab") && !text.contains("cdcd"), "{text}");
        assert!(!text.contains("171"), "{text}");

        // The rotation is over: only the current key.
        let c = Components::start().await;
        c.rejects(inbound(&click(&signed_id(48, &key_b()), 2)), "forged")
            .await;
    }
}

/// Story 7.2 S1-S4, S17 and S18: signed modal submits, the
/// `noctis.interaction` span of a component run, and the INT-11 follow-up
/// cap on components.
mod amendments {
    use noctis_codec::{Parts, encode};
    use noctis_ui::__macro::ModalFromPayload;
    use noctis_ui::{ModalForm, ModalSubmission, SubmitError, SubmittedValue};

    use super::*;
    use crate::{AutoDefer, ComponentResponder};

    /// The Payload of [`SigBio`]: one `u64`.
    #[derive(Debug)]
    struct SigBioId {
        item: u64,
    }

    impl Payload for SigBioId {
        const MAX_ENCODED_LEN: Option<usize> = Some(11);

        fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
            out.field(&self.item)
        }
    }

    impl PayloadDecode<'_> for SigBioId {
        fn decode_fields(f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
            Ok(Self { item: f.field()? })
        }
    }

    /// A `signed` modal with one Payload field and one required text input
    /// (`bio`): what `#[derive(Modal)]` with `signed` implements.
    #[derive(Debug)]
    struct SigBio {
        item: u64,
        bio: String,
    }

    impl noctis_codec::DeclaredRuntime for SigBio {}
    impl crate::__Guards<ModalSubmitCtx> for SigBio {}

    impl noctis_codec::Route for SigBio {
        const ID: noctis_codec::RouteId = noctis_codec::RouteId::new("sigbio");
        const SHAPE: RouteShape = RouteShape::INLINE.signed();
        const SCHEMA: u8 = 0;
        type Payload = SigBioId;
    }

    impl ModalForm for SigBio {
        fn from_submission(s: &ModalSubmission) -> Result<Self, SubmitError> {
            let payload = noctis_ui::__macro::decode_modal_payload::<Self>(s)?;
            Self::from_payload(payload, s)
        }
    }

    impl ModalFromPayload for SigBio {
        fn from_payload(payload: SigBioId, s: &ModalSubmission) -> Result<Self, SubmitError> {
            match s.get("bio") {
                Some(SubmittedValue::TextInput(text)) => Ok(Self {
                    item: payload.item,
                    bio: text.clone(),
                }),
                _ => Err(SubmitError::Missing { key: "bio".into() }),
            }
        }
    }

    /// The submit id of [`SigBio`] with `item`, tagged with `key`.
    fn sigbio_id(item: u64, key: &noctis_codec::PayloadKey) -> String {
        encode::<SigBio>(&SigBioId { item }, &Parts::new().with_signer(key))
            .unwrap()
            .to_string()
    }

    /// `parsed` clicked on the V2 source message 77 in channel 55.
    fn on_v2_message(parsed: &Value) -> Inbound {
        let mut raw = parsed.clone();
        raw["message"] =
            json!({ "id": "77", "channel_id": "55", "flags": 32768, "components": [] });
        inbound_raw(parsed, &raw)
    }

    /// Runs `inbound` through the component pipeline of `p`.
    async fn run(p: &Pipeline, inbound: Inbound) {
        run_component(
            Arc::clone(p.fx.app.registry()),
            Arc::clone(p.fx.app.shared()),
            inbound,
            CancellationToken::new(),
        )
        .await;
    }

    /// S1-S4: a signed modal decodes its `custom_id` once, with the key
    /// ring, and builds the struct from the verified Payload.
    #[tokio::test]
    async fn signed_modal_submit_verifies() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let log = Log::default();
        let seen = log.clone();
        let module = Module::new("m").__add(ModalHandler::__new::<SigBio, _, _>(
            "m::SigBio",
            "sigbio",
            move |v: SigBio, _: ModalSubmitCtx| {
                seen.push(format!("{}:{}", v.item, v.bio));
                async { Ok(Responded::new()) }
            },
        ));
        let p = Pipeline::start_with(module, |b| {
            b.payload_key(key_a()).previous_payload_key(key_b());
        })
        .await;
        let rejected =
            || capture.lines_with(&["noctis::dispatch", "component interaction rejected"]);

        // S1 and S2: the current and the previous key.
        run(&p, inbound(&submit(&sigbio_id(47, &key_a()), true))).await;
        run(&p, inbound(&submit(&sigbio_id(48, &key_b()), true))).await;
        assert_eq!(log.lines(), ["47:hi", "48:hi"]);
        assert!(rejected().is_empty(), "{}", capture.text());
        assert!(p.requests().is_empty(), "{:?}", p.requests());

        // S3: a flipped tag is `forged`, with the notice.
        let mut flipped = sigbio_id(47, &key_a());
        let last = flipped.pop().unwrap();
        flipped.push(if last == 'a' { 'b' } else { 'a' });
        run(&p, inbound(&submit(&flipped, true))).await;
        let lines = rejected();
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(lines[0].contains("reason=\"forged\""), "{}", lines[0]);
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_generic_notice(&requests[0]);

        // S4: a valid tag without the required input is `submit_invalid`,
        // never `undecodable`.
        p.fx.server.log.lock().unwrap().clear();
        run(&p, inbound(&submit(&sigbio_id(49, &key_a()), false))).await;
        let lines = rejected();
        assert_eq!(lines.len(), 2, "{}", capture.text());
        assert!(
            lines[1].contains("reason=\"submit_invalid\""),
            "{}",
            lines[1]
        );
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_generic_notice(&requests[0]);
        assert_eq!(log.lines().len(), 2);

        let text = capture.text();
        assert!(!text.contains("abab") && !text.contains("cdcd"), "{text}");
    }

    /// S17: the rejection, the auto-defer, the handler's own log and the
    /// answer all run inside `noctis.interaction`.
    #[tokio::test]
    async fn component_run_inside_interaction_span() {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        let module = Module::new("shop").__add(ComponentHandler::__new_with_responder::<
            Buy,
            AutoDefer,
            _,
            _,
            _,
        >(
            "shop::Buy",
            "shop.buy",
            ComponentKind::Button,
            crate::kind::__Response::Update,
            |_: Buy, _: ComponentCtx, _: ComponentResponder| async {
                tracing::info!("inside the click");
                tokio::time::sleep(Duration::from_millis(150)).await;
                Ok(v2("m"))
            },
        ));
        let p = Pipeline::start_with(module, |b| {
            b.payload_key(key_a())
                .ack_deadline(Duration::from_millis(50));
        })
        .await;

        run(&p, inbound(&click("0nope:", 2))).await;
        let lines = capture.lines_with(&["component interaction rejected"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(
            lines[0].contains("noctis.interaction{app=\"default\" shard=0"),
            "{}",
            lines[0]
        );
        assert!(!lines[0].contains("route=\"shop.buy\""), "{}", lines[0]);

        run(&p, on_v2_message(&click("0shop.buy:47", 2))).await;
        let span = "noctis.interaction{app=\"default\" shard=0 module=\"shop\" route=\"shop.buy\"}";
        for needle in ["inside the click", "auto-deferred at the ack deadline"] {
            let lines = capture.lines_with(&[needle]);
            assert_eq!(lines.len(), 1, "{needle}: {}", capture.text());
            assert!(lines[0].contains(span), "{needle}: {}", lines[0]);
        }
        let requests = p.requests();
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_eq!(requests[1].2, json!({ "type": 6 }));
        assert_eq!(requests[2].0, Method::PATCH);
        let text = capture.text();
        assert!(!text.contains("abab"), "{text}");
    }

    /// S18 (INT-11): a user-install-only click sends at most 5 follow-ups.
    #[tokio::test]
    async fn user_install_component_follow_up_cap() {
        let log = Log::default();
        let seen = log.clone();
        let module = Module::new("shop").__add(ComponentHandler::__new_with_responder::<
            Buy,
            AutoDefer,
            _,
            _,
            _,
        >(
            "shop::Buy",
            "shop.buy",
            ComponentKind::Button,
            crate::kind::__Response::Update,
            move |_: Buy, _: ComponentCtx, r: ComponentResponder| {
                let seen = seen.clone();
                async move {
                    let acked = r.defer_update().await?;
                    let mut results = Vec::new();
                    for _ in 0..6 {
                        let sent = acked.follow_up(v2("f"), Visibility::Ephemeral).await;
                        results.push(match sent {
                            Ok(_) => "ok",
                            Err(ResponderError::FollowupCap) => "cap",
                            Err(_) => "other",
                        });
                    }
                    seen.push(results.join(","));
                    Ok::<_, HandlerError>(acked.done())
                }
            },
        ));
        let p = Pipeline::start_with(module, |b| {
            b.payload_key(key_a());
        })
        .await;
        let mut json = click("0shop.buy:47", 2);
        json["authorizing_integration_owners"] = json!({ "1": "3" });
        run(&p, on_v2_message(&json)).await;
        assert_eq!(log.lines(), ["ok,ok,ok,ok,ok,cap"]);
        let requests = p.requests();
        assert_eq!(requests[0].2, json!({ "type": 6 }));
        let follow_ups = requests
            .iter()
            .filter(|r| r.1.starts_with("/api/v10/webhooks/7/tok?"))
            .count();
        assert_eq!(follow_ups, 5, "{requests:?}");
    }
}
