//! `App::run` sharding against `FakeDiscordHttp` and `FakeGateway`, and
//! `noctis_gateway::Gateway` directly (Story 5.4, Matrix 19-22 and 24-30;
//! Story 12.1 Shard Ranges).
//! Matrix 23 (the misalignment warning) lives in `sharding_warning.rs`, which
//! installs a global subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::collections::BTreeMap;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctis_core::{App, AppBuilder, Error, RunError, Runtime, StartError, Token};
use noctis_gateway::{
    EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundKind, InboundSink, ShardEvent,
};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_ports::{BoxFuture, IdentifyQueue, Lease, PortError};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FakeResponse, GatewayBot, IdentifyReply,
    Method, RecordedFrame, SessionStartLimit, StatusCode,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use twilight_gateway::{EventType, Intents, ShardId};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// The two fakes an App talks to.
struct Fakes {
    http: FakeDiscordHttp,
    gw: FakeGateway,
}

impl Fakes {
    async fn start(shards: u32, max_concurrency: u32) -> Self {
        Self::start_with(GatewayBot {
            shards,
            session_start_limit: SessionStartLimit {
                max_concurrency,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        })
        .await
    }

    async fn start_with(bot: GatewayBot) -> Self {
        Self {
            http: FakeDiscordHttp::start_with(bot).await.unwrap(),
            gw: FakeGateway::start().await.unwrap(),
        }
    }

    /// A builder on `Runtime::for_test()` with a token, wired to both fakes.
    fn builder(&self) -> AppBuilder {
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(self.http.url())
            .gateway_url(self.gw.url());
        builder
    }

    /// Every IDENTIFY received so far.
    fn identifies(&self) -> Vec<RecordedFrame> {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(2))
            .collect()
    }

    /// Polls until at least `n` IDENTIFYs arrived, within [`WAIT`].
    async fn wait_identifies(&self, n: usize) -> Vec<RecordedFrame> {
        let deadline = Instant::now() + WAIT;
        loop {
            let frames = self.identifies();
            if frames.len() >= n {
                return frames;
            }
            assert!(
                Instant::now() < deadline,
                "only {} of {n} identifies",
                frames.len()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The `GET /gateway/bot` requests received.
    fn gateway_bot_requests(&self) -> usize {
        self.http
            .requests()
            .iter()
            .filter(|r| r.method == Method::GET && r.path == "/api/v10/gateway/bot")
            .count()
    }
}

/// Runs `app` in its own task.
fn spawn(app: App) -> JoinHandle<Result<(), Error>> {
    tokio::spawn(app.run_with_args(std::iter::empty::<&str>()))
}

/// The result of `run`, within `within`.
async fn finished(handle: JoinHandle<Result<(), Error>>, within: Duration) -> Result<(), Error> {
    tokio::time::timeout(within, handle)
        .await
        .expect("run finished within the wait")
        .unwrap()
}

/// The `StartError` of a failed `run`.
async fn start_error(app: App) -> StartError {
    match finished(spawn(app), WAIT).await {
        Err(Error::Start(error)) => error,
        other => panic!("expected a StartError, got {other:?}"),
    }
}

/// `d.shard` of an IDENTIFY frame.
fn shard_of(frame: &RecordedFrame) -> (u64, u64) {
    let shard = &frame.json().unwrap()["d"]["shard"];
    (shard[0].as_u64().unwrap(), shard[1].as_u64().unwrap())
}

// ---- Matrix 19: default queue ----

#[tokio::test(flavor = "multi_thread")]
async fn gw_01_twenty_shards_identify_in_buckets_of_sixteen() {
    let fakes = Fakes::start(20, 16).await;
    let run = spawn(fakes.builder().build().unwrap());
    let frames = fakes.wait_identifies(20).await;
    let mut at = BTreeMap::new();
    for frame in &frames {
        let (shard, total) = shard_of(frame);
        assert_eq!(total, 20);
        assert!(
            at.insert(shard, frame.received_at).is_none(),
            "shard {shard} twice"
        );
    }
    assert_eq!(
        at.keys().copied().collect::<Vec<_>>(),
        (0..20).collect::<Vec<_>>()
    );
    assert_eq!(fakes.gw.connections().len(), 20);
    // Shards connect concurrently, so which shard of a bucket identifies
    // first is not fixed (0 or 16 for bucket 0): check per bucket. The first
    // round covers all 16 buckets within 2 s; the second shard of buckets
    // 0-3 waits the 5 s bucket interval.
    let first = *at.values().min().unwrap();
    for bucket in 0..16 {
        let round_one = if bucket < 4 {
            at[&bucket].min(at[&(bucket + 16)])
        } else {
            at[&bucket]
        };
        assert!(
            round_one - first <= Duration::from_secs(2),
            "bucket {bucket}"
        );
    }
    for bucket in 0..4 {
        let (a, b) = (at[&bucket], at[&(bucket + 16)]);
        assert!(
            a.max(b) - a.min(b) >= Duration::from_millis(4500),
            "bucket {bucket}"
        );
    }
    assert!(!run.is_finished());
    run.abort();
}

// ---- Matrix 20: the bot's IdentifyQueue port ----

/// Records each bucket asked for, then delegates.
struct Recording {
    inner: MemoryIdentifyQueue,
    buckets: Arc<Mutex<Vec<u32>>>,
}

impl IdentifyQueue for Recording {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
        self.buckets.lock().unwrap().push(bucket);
        self.inner.acquire(bucket)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn gw_01_identify_goes_through_the_port() {
    let fakes = Fakes::start(20, 16).await;
    let buckets = Arc::new(Mutex::new(Vec::new()));
    let app = fakes
        .builder()
        .identify_queue(Recording {
            inner: MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap()),
            buckets: Arc::clone(&buckets),
        })
        .build()
        .unwrap();
    let run = spawn(app);
    let frames = fakes.wait_identifies(20).await;
    let mut shards: Vec<u32> = frames
        .iter()
        .map(|f| u32::try_from(shard_of(f).0).unwrap() % 16)
        .collect();
    let mut recorded = buckets.lock().unwrap().clone();
    shards.sort_unstable();
    recorded.sort_unstable();
    assert_eq!(recorded, shards);
    assert_eq!(recorded.len(), 20);
    run.abort();
}

// ---- Matrix 21: one shard ----

#[tokio::test(flavor = "multi_thread")]
async fn single_shard_identifies_and_keeps_running() {
    let fakes = Fakes::start(1, 1).await;
    let run = spawn(fakes.builder().build().unwrap());
    let frames = fakes.wait_identifies(1).await;
    let d = frames[0].json().unwrap()["d"].clone();
    assert_eq!(d["shard"], json!([0, 1]));
    assert_eq!(d["token"], "Bot fake-token");
    assert_eq!(d["intents"], Intents::GUILDS.bits());
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!run.is_finished());
    assert_eq!(fakes.gw.connections().len(), 1);
    assert_eq!(fakes.gateway_bot_requests(), 1);
    run.abort();
}

// ---- Matrix 22, 24: plan refusals ----

#[tokio::test(flavor = "multi_thread")]
async fn gw_03_fixed_total_too_small() {
    let fakes = Fakes::start(20, 1).await;
    let app = fakes.builder().shard_total(4).build().unwrap();
    assert_eq!(
        start_error(app).await,
        StartError::ShardTotalTooSmall {
            total: 4,
            minimum: 8
        }
    );
    assert!(fakes.gw.connections().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn gw_02_session_start_limit() {
    let fakes = Fakes::start_with(GatewayBot {
        shards: 20,
        session_start_limit: SessionStartLimit {
            remaining: 19,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    })
    .await;
    let error = start_error(fakes.builder().build().unwrap()).await;
    assert!(
        matches!(
            error,
            StartError::SessionStartLimit {
                required: 20,
                remaining: 19,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(fakes.gw.connections().is_empty());
}

// ---- Matrix 25: GET /gateway/bot failures ----

#[tokio::test(flavor = "multi_thread")]
async fn gateway_bot_failures() {
    let api_error = |message: &str| json!({ "code": 0, "message": message });
    let bot = |shards: u32, max_concurrency: u32| {
        GatewayBot {
            shards,
            session_start_limit: SessionStartLimit {
                max_concurrency,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        }
        .to_json()
    };
    let cases: Vec<(FakeResponse, Option<Duration>, StartError)> = vec![
        (
            FakeResponse::json(StatusCode::UNAUTHORIZED, &api_error("401: Unauthorized")),
            None,
            StartError::InvalidToken,
        ),
        (
            FakeResponse::json(StatusCode::INTERNAL_SERVER_ERROR, &api_error("boom")),
            None,
            StartError::GatewayInfo { status: Some(500) },
        ),
        (
            FakeResponse::json(StatusCode::OK, &json!({})),
            None,
            StartError::GatewayInfo { status: None },
        ),
        (
            FakeResponse::json(StatusCode::OK, &bot(1, 1)).delay(Duration::from_secs(2)),
            Some(Duration::from_millis(100)),
            StartError::GatewayInfo { status: None },
        ),
        (
            FakeResponse::json(StatusCode::OK, &bot(1, 70_000)),
            None,
            StartError::GatewayInfo { status: None },
        ),
        (
            FakeResponse::json(StatusCode::OK, &bot(0, 1)),
            None,
            StartError::InvalidGatewayInfo,
        ),
        (
            FakeResponse::json(StatusCode::OK, &bot(1, 0)),
            None,
            StartError::InvalidGatewayInfo,
        ),
    ];
    for (response, timeout, expected) in cases {
        let fakes = Fakes::start(1, 1).await;
        fakes.http.route(Method::GET, "/gateway/bot", response);
        let mut builder = fakes.builder();
        if let Some(timeout) = timeout {
            builder.http_timeout(timeout);
        }
        let error = start_error(builder.build().unwrap()).await;
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("fake-token"));
        assert!(fakes.gw.connections().is_empty(), "{expected:?}");
    }
}

// ---- Matrix 26: fatal close at identify ----

#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_at_identify_fails_startup() {
    for (code, expected) in [
        (4014, StartError::DisallowedIntents { shard: 0 }),
        (4004, StartError::InvalidToken),
        (4013, StartError::InvalidIntents { shard: 0 }),
        (
            4011,
            StartError::FatalClose {
                shard: 0,
                code: 4011,
            },
        ),
    ] {
        let fakes = Fakes::start(1, 1).await;
        fakes.gw.set_config(FakeGatewayConfig {
            identify: IdentifyReply::Close(code),
            ..FakeGatewayConfig::default()
        });
        let run = spawn(fakes.builder().build().unwrap());
        match finished(run, Duration::from_secs(10)).await {
            Err(Error::Start(error)) => assert_eq!(error, expected),
            other => panic!("{code}: {other:?}"),
        }
        assert_eq!(fakes.gw.connections().len(), 1, "{code}");
    }
}

// ---- Matrix 27: resumable close at identify ----

#[tokio::test(flavor = "multi_thread")]
async fn resumable_close_at_identify_reconnects() {
    let fakes = Fakes::start(1, 1).await;
    fakes.gw.set_config(FakeGatewayConfig {
        identify: IdentifyReply::Close(4000),
        ..FakeGatewayConfig::default()
    });
    let run = spawn(fakes.builder().build().unwrap());
    fakes.wait_identifies(1).await;
    // Flip only once the first connection is closed, so its IDENTIFY surely
    // got the close.
    let deadline = Instant::now() + WAIT;
    while fakes
        .gw
        .connections()
        .first()
        .is_none_or(|c| c.closed_at.is_none())
    {
        assert!(Instant::now() < deadline, "first connection still open");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fakes.gw.set_config(FakeGatewayConfig::default());
    fakes.wait_identifies(2).await;
    // READY follows the second IDENTIFY; give it a moment to arrive.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(fakes.gw.connections().len() >= 2);
    assert!(!run.is_finished());
    run.abort();
}

// ---- Matrix 28: fatal close after startup ----

#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_after_startup_ends_run() {
    let fakes = Fakes::start(2, 2).await;
    let run = spawn(fakes.builder().build().unwrap());
    fakes.wait_identifies(2).await;
    // READY is sent right after each IDENTIFY; let both shards see it.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!run.is_finished());
    assert_eq!(fakes.gw.close(4014, ""), Ok(2));
    // Story 5.8: every shard ending is loud, not a silent `Ok(())`.
    assert!(matches!(
        finished(run, Duration::from_secs(5)).await,
        Err(Error::Run(RunError::AllShardsEnded))
    ));
}

// ---- Story 12.1: Shard Ranges ----

/// The `GatewayBot` of 4 recommended shards with `remaining` session starts.
fn four_shards(remaining: u32) -> GatewayBot {
    GatewayBot {
        shards: 4,
        session_start_limit: SessionStartLimit {
            remaining,
            max_concurrency: 2,
            ..SessionStartLimit::default()
        },
        ..GatewayBot::default()
    }
}

/// G1, G3, P7: an App with range `2..4` of 4 opens two connections,
/// identifies only `[2, 4]` and `[3, 4]`, finishes startup on their READY
/// (the run keeps going) and needs only 2 session starts.
#[tokio::test(flavor = "multi_thread")]
async fn shard_range_runs_only_its_shards() {
    let fakes = Fakes::start_with(four_shards(2)).await;
    let app = fakes
        .builder()
        .shard_total(4)
        .shard_range(2..4)
        .build()
        .unwrap();
    let run = spawn(app);
    let frames = fakes.wait_identifies(2).await;
    // Startup is done once both are READY; give the shards a moment.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut shards: Vec<(u64, u64)> = fakes.identifies().iter().map(shard_of).collect();
    shards.sort_unstable();
    assert_eq!(shards, [(2, 4), (3, 4)], "{frames:?}");
    assert_eq!(fakes.gw.connections().len(), 2);
    assert!(!run.is_finished());
    run.abort();
}

/// P8: the session start limit is checked against the range's 2 shards.
#[tokio::test(flavor = "multi_thread")]
async fn gw_02_session_start_limit_counts_the_range() {
    let fakes = Fakes::start_with(four_shards(1)).await;
    let app = fakes
        .builder()
        .shard_total(4)
        .shard_range(2..4)
        .build()
        .unwrap();
    let error = start_error(app).await;
    assert!(
        matches!(
            error,
            StartError::SessionStartLimit {
                required: 2,
                remaining: 1,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(fakes.gw.connections().is_empty());
}

/// G5: once both shards of the range end after startup, the run fails with
/// `AllShardsEnded`.
#[tokio::test(flavor = "multi_thread")]
async fn shard_range_all_ended_ends_run() {
    let fakes = Fakes::start_with(four_shards(1000)).await;
    let app = fakes
        .builder()
        .shard_total(4)
        .shard_range(2..4)
        .build()
        .unwrap();
    let run = spawn(app);
    fakes.wait_identifies(2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!run.is_finished());
    assert_eq!(fakes.gw.close(4014, ""), Ok(2));
    assert!(matches!(
        finished(run, Duration::from_secs(5)).await,
        Err(Error::Run(RunError::AllShardsEnded))
    ));
}

/// G1, G2: `Gateway::start` with `shards: 2..4` runs exactly those two
/// shards as `[2, 4]` and `[3, 4]`; an invalid range starts nothing.
#[tokio::test(flavor = "multi_thread")]
async fn gateway_starts_only_its_shard_range() {
    let gw = FakeGateway::start().await.unwrap();
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
    let options = |shards: std::ops::Range<u32>| GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(gw.url()),
        event_types: EventTypeFlags::INTERACTION_CREATE,
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::new(2).unwrap())),
        shards,
        total: NonZeroU32::new(4).unwrap(),
        max_concurrency: NonZeroU16::new(2).unwrap(),
        close_timeout: Duration::from_secs(5),
    };
    let (tx, _inbound) = mpsc::unbounded_channel();
    let sink = Arc::new(ChannelSink(tx));
    for bad in [2..2, 3..5] {
        assert!(matches!(
            Gateway::start(options(bad), sink.clone()),
            Err(noctis_gateway::ShardConfigError::InvalidShardRange)
        ));
    }
    assert!(gw.connections().is_empty());
    let (gateway, mut events) = Gateway::start(options(2..4), sink).unwrap();
    assert_eq!(gateway.shard_range(), 2..4);
    assert_eq!(gateway.shard_total(), 4);
    let mut ready = Vec::new();
    while ready.len() < 2 {
        if let Some(ShardEvent::Ready { shard }) = recv(&mut events).await {
            ready.push(shard);
        }
    }
    ready.sort_unstable();
    assert_eq!(ready, [2, 3]);
    let mut shards: Vec<(u64, u64)> = gw
        .frames()
        .iter()
        .filter(|f| f.op() == Some(2))
        .map(shard_of)
        .collect();
    shards.sort_unstable();
    assert_eq!(shards, [(2, 4), (3, 4)]);
    assert_eq!(gw.connections().len(), 2);
    drop(gateway);
}

// ---- Matrix 29: missing token ----

#[tokio::test(flavor = "multi_thread")]
async fn missing_token_makes_no_request() {
    let fakes = Fakes::start(1, 1).await;
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .http_proxy(fakes.http.url())
        .gateway_url(fakes.gw.url())
        .build()
        .unwrap();
    assert_eq!(start_error(app).await, StartError::MissingToken);
    assert!(fakes.http.requests().is_empty());
    assert!(fakes.gw.connections().is_empty());
}

// ---- Matrix 30: Gateway directly ----

/// Forwards every item to a channel.
struct ChannelSink(mpsc::UnboundedSender<Inbound>);

impl InboundSink for ChannelSink {
    fn push(&self, inbound: Inbound) {
        let _ = self.0.send(inbound);
    }
}

async fn recv<T>(rx: &mut mpsc::UnboundedReceiver<T>) -> Option<T> {
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .expect("item within the wait")
}

#[tokio::test(flavor = "multi_thread")]
async fn gateway_start_parses_into_the_sink() {
    let gw = FakeGateway::start().await.unwrap();
    // Building an App installs the rustls provider twilight's connector needs.
    App::builder()
        .runtime(&Runtime::for_test())
        .build()
        .unwrap();
    let (tx, mut inbound) = mpsc::unbounded_channel();
    let options = GatewayOptions {
        app: Arc::from("default"),
        token: "fake-token".to_owned(),
        intents: Intents::GUILDS,
        gateway_url: Some(gw.url()),
        event_types: EventTypeFlags::all(),
        identify_queue: Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN)),
        shards: 0..1,
        total: NonZeroU32::MIN,
        max_concurrency: NonZeroU16::MIN,
        close_timeout: Duration::from_secs(5),
    };
    let (gateway, mut events) = Gateway::start(options, Arc::new(ChannelSink(tx))).unwrap();
    assert_eq!(gateway.shard_total(), 1);
    assert_eq!(
        recv(&mut events).await,
        Some(ShardEvent::Ready { shard: 0 })
    );
    // READY itself is a wanted dispatch: skip it.
    let ready = recv(&mut inbound).await.unwrap();
    assert!(matches!(&ready.kind, InboundKind::Event(e) if e.kind() == EventType::Ready));

    let d = "{ \"id\": \"5\", \"application_id\": \"1\", \"type\": 1, \"token\": \"t\", \
             \"version\": 1, \"authorizing_integration_owners\": {}, \"entitlements\": [], \
             \"x\": \"\u{fc}\u{1f642}\u{fc}\" }";
    let before = tokio::time::Instant::now();
    assert_eq!(
        gw.send_raw(format!(
            r#"{{"op":0,"s":2,"t":"INTERACTION_CREATE","d":{d}}}"#
        )),
        1
    );
    let item = recv(&mut inbound).await.unwrap();
    let after = tokio::time::Instant::now();
    assert_eq!(item.shard, ShardId::ONE);
    assert!(before <= item.received_at && item.received_at <= after);
    let InboundKind::Interaction(interaction, raw) = item.kind else {
        panic!("not an interaction");
    };
    assert_eq!(interaction.id.get(), 5);
    assert_eq!(raw.as_bytes(), d.as_bytes());

    gw.dispatch(
        "MESSAGE_DELETE",
        json!({ "id": "3", "channel_id": "2" }) as Value,
    );
    let item = recv(&mut inbound).await.unwrap();
    assert!(matches!(&item.kind, InboundKind::Event(e) if e.kind() == EventType::MessageDelete));

    drop(gateway);
    while recv(&mut events).await.is_some() {}
}
