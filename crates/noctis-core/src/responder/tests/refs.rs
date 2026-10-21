//! Story 7.5 Matrix 9-19: the Responder mints and stores the Ref of every
//! Ref slot before it sends anything.

use std::fmt;

use noctis_codec::{
    CodecError, FieldReader, FieldWriter, Payload, PayloadDecode, RouteRuntime, RouteShape,
    SelectRoute,
};
use noctis_ui::__macro::{ButtonDefaults, SelectDefaults, build_button, build_user_select};
use noctis_ui::{ButtonStyle, ModalLabel, TextInput, TextInputStyle};

use super::*;
use crate::test_support::{TestRefs, test_route};

test_route!(Stored, "stored", RouteShape::BY_REF, 0);
test_route!(
    Quick,
    "quick",
    RouteShape::BY_REF,
    0,
    RouteRuntime::EMPTY.with_ttl_secs(900).one_shot()
);
test_route!(Picked, "picked", RouteShape::BY_REF.select(), 2);
test_route!(Spilled, "spill", RouteShape::SPILL, 0);
test_route!(RefForm, "ref.form", RouteShape::BY_REF, 0);

impl SelectRoute for Picked {}

/// A `spill` Route whose Payload is a free-text note: inline while it fits.
#[derive(Debug)]
struct Note {
    text: String,
}

impl Payload for Note {
    const MAX_ENCODED_LEN: Option<usize> = None;
    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
        out.field(&self.text)
    }
}

impl PayloadDecode<'_> for Note {
    fn decode_fields(f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
        Ok(Self { text: f.field()? })
    }
}

impl noctis_codec::Route for Note {
    const ID: noctis_codec::RouteId = noctis_codec::RouteId::new("note");
    const SHAPE: RouteShape = RouteShape::SPILL;
    const SCHEMA: u8 = 0;
    type Payload = Self;
}

impl noctis_codec::DeclaredRuntime for Note {}
impl noctis_ui::Handled for Note {}

const DAY: Duration = Duration::from_hours(24);

fn button<R: noctis_ui::__macro::ComponentValue>(route: R) -> noctis_ui::RouteButton<R> {
    build_button(
        route,
        &ButtonDefaults {
            style: ButtonStyle::Primary,
            label: Some("b"),
            emoji: None,
            disabled: false,
        },
    )
}

fn source() -> SourceMessage {
    SourceMessage {
        id: Id::new(77),
        channel_id: Id::new(55),
        format: MessageFormat::V2,
        ephemerality: Ephemerality::Public,
    }
}

/// A message of one action row of the button array `$buttons`.
macro_rules! row {
    ($buttons:expr) => {
        V2Message::new(Components::new().push(ActionRow::buttons($buttons)))
    };
}

/// The `custom_id`s of the first action row of the last request's `data`.
fn sent_ids(fx: &Fixture) -> Vec<String> {
    let requests = fx.requests();
    let data = &requests.last().expect("a request").2["data"];
    data["components"][0]["components"]
        .as_array()
        .expect("an action row")
        .iter()
        .map(|c| c["custom_id"].as_str().expect("an id").to_owned())
        .collect()
}

/// The Ref of a by-Ref `custom_id`: the 11 characters after `~`.
fn ref_of(id: &str) -> &str {
    id.split_once('~').expect("a Ref id").1
}

async fn update(fx: &Fixture, msg: V2Message) -> Result<(), ResponderError> {
    let shared = fx.component(Declared::Update, Some(source()));
    Responder::<Component>::new(shared)
        .update(msg)
        .await
        .map(|acked| drop(acked.done()))
}

/// Matrix 9: one `by_ref` button is stored under `default:ref:<ref>` with
/// the App's 24 h TTL, before the one callback.
#[tokio::test]
async fn mint_stores_the_record_before_the_callback() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    update(&fx, row!([button(Stored { item: 1 })]))
        .await
        .unwrap();
    let ids = sent_ids(&fx);
    let r = ref_of(&ids[0]);
    assert!(ids[0].starts_with("0stored:~"), "{ids:?}");
    assert_eq!(r.len(), 11);
    assert!(r.bytes().all(|b| b.is_ascii_alphanumeric()), "{r}");
    let key = format!("default:ref:{r}");
    assert_eq!(
        refs.calls(),
        [crate::test_support::RefCall {
            op: "put_if_absent",
            key: key.clone(),
            ttl: Some(DAY),
        }]
    );
    assert_eq!(refs.value(&key).as_deref(), Some(&b"0stored:1"[..]));
    assert_eq!(fx.requests().len(), 1);
}

/// Matrix 10: a Route's `ttl` wins over the App's; `.ref_ttl` sets the
/// App's.
#[tokio::test]
async fn route_ttl_and_app_ttl() {
    let refs = TestRefs::new();
    let hour = Duration::from_hours(1);
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), Some(hour)).await;
    update(&fx, row!([button(Quick { item: 1 })]))
        .await
        .unwrap();
    update(&fx, row!([button(Stored { item: 1 })]))
        .await
        .unwrap();
    let ttls: Vec<_> = refs.calls().into_iter().map(|c| c.ttl).collect();
    assert_eq!(ttls, [Some(Duration::from_mins(15)), Some(hour)]);
}

/// Matrix 11 and 12: equal slots share one Ref within a send (two equal
/// components still break CID-02 at validation, after minting); another
/// send mints new Refs.
#[tokio::test]
async fn equal_slots_share_one_ref_per_send() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let shared = fx.component(Declared::Update, Some(source()));
    let mint = |mut msg: V2Message| {
        let shared = Arc::clone(&shared);
        async move {
            shared.resolve_ids(msg.custom_ids_mut()).await.unwrap();
            msg.custom_ids_mut()
                .map(|id| id.get().unwrap().to_owned())
                .collect::<Vec<_>>()
        }
    };
    let msg = || {
        row!([
            button(Stored { item: 1 }),
            button(Stored { item: 1 }),
            button(Stored { item: 2 }),
        ])
    };
    let first = mint(msg()).await;
    assert_eq!(first[0], first[1]);
    assert_ne!(first[0], first[2]);
    assert_eq!(refs.calls_of("put_if_absent").len(), 2);
    let second = mint(msg()).await;
    assert_ne!(first[0], second[0]);
    assert_ne!(first[2], second[2]);
    assert_eq!(refs.calls_of("put_if_absent").len(), 4);

    // Sent, the shared id is a CID-02 duplicate: minted, then rejected.
    let err = update(&fx, msg()).await.unwrap_err();
    assert!(err.to_string().contains("CID-02"), "{err}");
    assert_eq!(refs.calls_of("put_if_absent").len(), 6);
    assert!(fx.requests().is_empty());
    // Two sends of one Payload get two Refs.
    update(&fx, row!([button(Stored { item: 1 })]))
        .await
        .unwrap();
    let a = sent_ids(&fx);
    update(&fx, row!([button(Stored { item: 1 })]))
        .await
        .unwrap();
    assert_ne!(a, sent_ids(&fx));
}

/// Matrix 13: a collision mints again; the second Ref is used.
#[tokio::test]
async fn a_collision_mints_again() {
    let refs = TestRefs::new();
    refs.script_puts([Some(false)]);
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    update(&fx, row!([button(Stored { item: 1 })]))
        .await
        .unwrap();
    let puts = refs.calls_of("put_if_absent");
    assert_eq!(puts.len(), 2);
    assert_ne!(puts[0].key, puts[1].key);
    let ids = sent_ids(&fx);
    assert_eq!(puts[1].key, format!("default:ref:{}", ref_of(&ids[0])));
}

/// Matrix 14 and 15: three collisions are `RefCollision`, a store error is
/// `RefStore`; nothing is sent and the interaction stays Pending.
#[tokio::test]
async fn exhausted_and_failing_stores_send_nothing() {
    let refs = TestRefs::new();
    refs.script_puts([Some(false), Some(false), Some(false)]);
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let shared = fx.component(Declared::Update, Some(source()));
    let err = Responder::<Component>::new(Arc::clone(&shared))
        .update(row!([button(Stored { item: 1 })]))
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::RefCollision), "{err}");
    assert_eq!(
        err.to_string(),
        "could not mint a unique Ref after 3 attempts"
    );
    assert_eq!(refs.calls_of("put_if_absent").len(), 3);
    assert!(fx.requests().is_empty());
    assert_eq!(shared.state(), PENDING);

    refs.script_puts([None]);
    let shared = fx.component(Declared::Update, Some(source()));
    let err = Responder::<Component>::new(Arc::clone(&shared))
        .reply(row!([button(Stored { item: 1 })]))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ResponderError::RefStore(PortError::Backend(_))),
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "the Ref could not be stored: port backend failed"
    );
    assert!(fx.requests().is_empty());
    assert_eq!(shared.state(), PENDING);
}

/// Matrix 16: a `spill` Route is inline while the value fits (no store
/// call) and by Ref otherwise (one call).
#[tokio::test]
async fn spill_mints_only_when_it_does_not_fit() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    update(&fx, row!([button(Spilled { item: 3 })]))
        .await
        .unwrap();
    assert_eq!(sent_ids(&fx), ["0spill:3"]);
    update(
        &fx,
        row!([button(Note {
            text: "short".to_owned(),
        })]),
    )
    .await
    .unwrap();
    assert_eq!(sent_ids(&fx), ["0note:short"]);
    assert!(refs.calls().is_empty());
    update(
        &fx,
        row!([button(Note {
            text: "x".repeat(200),
        })]),
    )
    .await
    .unwrap();
    let ids = sent_ids(&fx);
    assert!(ids[0].starts_with("0note:~"), "{ids:?}");
    let puts = refs.calls_of("put_if_absent");
    assert_eq!(puts.len(), 1);
    assert_eq!(
        refs.value(&puts[0].key).map(|v| v.len()),
        Some("0note:".len() + 200)
    );
}

/// Matrix 17: a `by_ref` select with schema 2 carries `S` and `N`.
#[tokio::test]
async fn ref_select_carries_schema_and_nonce() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let defaults = SelectDefaults {
        placeholder: Some("p"),
        min_values: None,
        max_values: None,
        disabled: false,
    };
    let msg = V2Message::new(Components::new().push(ActionRow::select(build_user_select(
        Picked { item: 1 },
        &defaults,
    ))));
    update(&fx, msg).await.unwrap();
    let id = sent_ids(&fx).remove(0);
    // F = S (1) + N (4) = 5, then the schema digit and the two-character
    // nonce, then `~` and the Ref.
    assert!(id.starts_with("5picked:2"), "{id}");
    assert_eq!(id.len(), "5picked:2".len() + 2 + 1 + 11, "{id}");
    assert_eq!(id.as_bytes()["5picked:2".len() + 2], b'~', "{id}");
    assert!(id.len() <= 41);
    let key = &refs.calls_of("put_if_absent")[0].key;
    assert_eq!(refs.value(key).as_deref(), Some(&b"1picked:21"[..]));
}

/// Matrix 18: a `by_ref` modal's `custom_id` is a Ref id, stored before
/// the type-9 callback.
#[tokio::test]
async fn show_modal_mints_the_modal_ref() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let shared = fx.component(Declared::Update, Some(source()));
    let modal = noctis_ui::__macro::build_modal::<RefForm>(RefForm { item: 4 }, "Form").push(
        ModalLabel::new("Bio", TextInput::new("bio", TextInputStyle::Short)),
    );
    let manual: ComponentResponder<Manual> = Responder::new(shared);
    let _ = manual.show_modal(modal).await.unwrap().done();
    let requests = fx.requests();
    assert_eq!(requests[0].2["type"], 9);
    let id = requests[0].2["data"]["custom_id"].as_str().unwrap();
    assert!(id.starts_with("0ref.form:~"), "{id}");
    let key = format!("default:ref:{}", ref_of(id));
    assert_eq!(refs.value(&key).as_deref(), Some(&b"0ref.form:4"[..]));
}

/// Matrix 19: the detached transport mints and finishes without a store.
#[tokio::test]
async fn detached_mints_without_storing() {
    let shared = Shared::declared(
        Transport::Detached,
        Declared::Update,
        None,
        Limits::default(),
        None,
    );
    shared
        .finish(row!([button(Stored { item: 1 })]).into())
        .await
        .unwrap();
}

/// An already-resolved id is never minted again: preparing the same
/// message twice keeps its Ref and stores it once.
#[tokio::test]
async fn a_resolved_id_is_not_minted_again() {
    let refs = TestRefs::new();
    let fx = Fixture::start_refs(discord(), Arc::clone(&refs), None).await;
    let shared = fx.component(Declared::Update, Some(source()));
    let mut msg: OutgoingMessage = row!([button(Stored { item: 1 })]).into();
    shared.prepare(&mut msg).await.unwrap();
    let id = |msg: &mut OutgoingMessage| match msg {
        OutgoingMessage::V2(m) => m.custom_ids_mut().next().unwrap().get().unwrap().to_owned(),
        OutgoingMessage::Legacy(_) => unreachable!("a V2 message"),
    };
    let first = id(&mut msg);
    shared.prepare(&mut msg).await.unwrap();
    assert_eq!(id(&mut msg), first);
    assert_eq!(refs.calls_of("put_if_absent").len(), 1);
}
