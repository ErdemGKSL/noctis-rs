//! Story 5.7 Matrix 30-33 and 35: the metric catalog and label policy.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use metrics::Unit;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use metrics_util::{CompositeKey, MetricKind};
use noctis_gateway::{Inbound, InboundKind, InboundSink};
use serde_json::json;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::event::Event;
use twilight_model::gateway::payload::incoming::{MessageDelete, TypingStart};
use twilight_model::id::Id;

use super::*;
use crate::app::ShardStatus;
use crate::test_support::{Recording, handler};
use crate::worker::drain;
use crate::{App, ErrorOrigin, HandlerError, Module, Runtime};

/// Fixture snowflakes: guild, channel, message and user ids.
const GA: u64 = 1_234_567_890_123_456_789;
const CHANNEL: u64 = 1_111_111_111_111_111_111;
const USER: u64 = 1_222_222_222_222_222_222;
const FIXTURES: [u64; 3] = [GA, CHANNEL, USER];

/// Every label name a catalog row may use.
const ALLOWED_LABELS: [&str; 10] = [
    "app",
    "lane",
    "event_type",
    "kind",
    "shard",
    "status_class",
    "phase",
    "reason",
    "reused",
    "outcome",
];
/// Substrings no label name may contain.
const FORBIDDEN_IN_NAMES: [&str; 8] = [
    "id", "user", "guild", "channel", "message", "name", "content", "error",
];

fn kind_of(metric_type: MetricType) -> MetricKind {
    match metric_type {
        MetricType::Counter => MetricKind::Counter,
        MetricType::Gauge => MetricKind::Gauge,
        MetricType::Histogram => MetricKind::Histogram,
    }
}

/// Matrix 32: the static catalog obeys the label policy.
#[test]
fn catalog_static() {
    assert_eq!(METRICS.len(), 15);
    let names: HashSet<_> = METRICS.iter().map(|m| m.name).collect();
    assert_eq!(names.len(), 15);
    for info in METRICS {
        assert!(info.name.starts_with("noctis_"), "{}", info.name);
        assert_eq!(info.labels.first(), Some(&"app"), "{}", info.name);
        assert!(!info.description.is_empty());
        for label in info.labels {
            assert!(ALLOWED_LABELS.contains(label), "{}: {label}", info.name);
            for forbidden in FORBIDDEN_IN_NAMES {
                assert!(!label.contains(forbidden), "{}: {label}", info.name);
            }
        }
        if info.name.ends_with("_total") {
            assert_eq!(info.metric_type, MetricType::Counter, "{}", info.name);
        }
        if info.name.ends_with("_seconds") {
            assert_eq!(info.metric_type, MetricType::Histogram, "{}", info.name);
        }
    }
    let rows: Vec<_> = METRICS.iter().map(|m| (m.name, m.labels)).collect();
    assert_eq!(
        rows,
        [
            ("noctis_lane_depth", &["app", "lane"][..]),
            ("noctis_event_overflow_depth", &["app"][..]),
            ("noctis_events_dropped_total", &["app", "event_type"][..]),
            ("noctis_handler_duration_seconds", &["app", "kind"][..]),
            ("noctis_handler_errors_total", &["app", "kind"][..]),
            ("noctis_autodefer_total", &["app"][..]),
            ("noctis_ack_latency_seconds", &["app"][..]),
            ("noctis_shard_status", &["app", "shard"][..]),
            ("noctis_http_requests_total", &["app", "status_class"][..]),
            ("noctis_http_phase_seconds", &["app", "phase"][..]),
            ("noctis_http_connections_total", &["app", "reason"][..]),
            ("noctis_http_conn_reused_total", &["app", "reused"][..]),
            ("noctis_http_h2_ping_rtt_seconds", &["app"][..]),
            ("noctis_http_ping_timeouts_total", &["app"][..]),
            (
                "noctis_interaction_ack_hedges_total",
                &["app", "outcome"][..]
            ),
        ]
    );
}

/// Checks one recorded label value against the policy.
fn check_value(metric: &str, label: &str, value: &str, total: u32) {
    match label {
        "app" => assert_eq!(value, "default", "{metric}"),
        "lane" => assert!(
            ["interaction", "event"].contains(&value),
            "{metric}: {value}"
        ),
        "kind" => assert_eq!(value, "event", "{metric}"),
        "event_type" => assert!(
            (1..=64).contains(&value.len())
                && value.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'),
            "{metric}: {value}"
        ),
        "status_class" => assert!(
            ["1xx", "2xx", "3xx", "4xx", "5xx", "error"].contains(&value),
            "{metric}: {value}"
        ),
        "phase" => assert!(
            ["dns", "connect", "tls", "ttfb", "total"].contains(&value),
            "{metric}: {value}"
        ),
        "reason" => assert!(
            ["warmup", "redial", "hedge", "demand"].contains(&value),
            "{metric}: {value}"
        ),
        "reused" => assert!(["true", "false"].contains(&value), "{metric}: {value}"),
        "outcome" => assert!(
            ["won", "lost", "40060"].contains(&value),
            "{metric}: {value}"
        ),
        "shard" => {
            let shard: u32 = value
                .parse()
                .unwrap_or_else(|_| panic!("{metric}: {value}"));
            assert!(shard < total, "{metric}: {value}");
        }
        other => panic!("{metric}: unexpected label {other}"),
    }
    let digits = value.bytes().filter(u8::is_ascii_digit).count();
    assert!(digits < 15, "{metric}: {label}={value}");
    for fixture in FIXTURES {
        assert_ne!(value, fixture.to_string(), "{metric}: {label}");
    }
}

/// Checks every key of a snapshot against [`METRICS`]; returns the names.
fn check_snapshot(
    snapshot: &[(
        CompositeKey,
        Option<Unit>,
        Option<metrics::SharedString>,
        DebugValue,
    )],
    total: u32,
) -> HashSet<String> {
    let mut seen = HashSet::new();
    for (key, ..) in snapshot {
        let name = key.key().name();
        let info = METRICS
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} is not in the catalog"));
        assert_eq!(key.kind(), kind_of(info.metric_type), "{name}");
        let labels: Vec<_> = key.key().labels().map(metrics::Label::key).collect();
        assert_eq!(labels, info.labels, "{name}");
        for label in key.key().labels() {
            check_value(name, label.key(), label.value(), total);
        }
        seen.insert(name.to_owned());
    }
    seen
}

fn delete(id: u64) -> Event {
    Event::MessageDelete(MessageDelete {
        channel_id: Id::new(CHANNEL),
        guild_id: Some(Id::new(GA)),
        id: Id::new(id),
    })
}

fn typing() -> Event {
    let typing: TypingStart = serde_json::from_value(json!({
        "channel_id": CHANNEL.to_string(),
        "guild_id": GA.to_string(),
        "timestamp": 0,
        "user_id": USER.to_string(),
    }))
    .unwrap();
    Event::TypingStart(Box::new(typing))
}

fn inbound(event: Event) -> Inbound {
    Inbound {
        shard: ShardId::new(1, 2),
        received_at: tokio::time::Instant::now(),
        kind: InboundKind::Event(Box::new(event)),
    }
}

/// Polls `cond` within 10 s.
async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Matrix 33: lanes, a worker run and the status updater record only
/// catalog metrics with bounded label values.
#[tokio::test]
async fn recorded_set_core() {
    let recorder = DebuggingRecorder::new();
    let snapshotter: Snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let recording = Arc::new(Recording::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let mixed = {
        let runs = Arc::clone(&runs);
        handler("bot::mixed", false, move |_, e: MessageDelete| {
            runs.fetch_add(1, Ordering::SeqCst);
            async move {
                match e.id.get() % 3 {
                    0 => Err(HandlerError::new(std::io::Error::other("x"))),
                    1 => panic!("boom"),
                    _ => Ok(()),
                }
            }
        })
    };
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .event_lane(1, 0)
        .error_sink(ErrorOrigin::Event, Arc::clone(&recording))
        .module(Module::new("m").__add(mixed))
        .build()
        .unwrap();
    let (sender, receiver) = noctis_gateway::lanes(app.lane_config());
    // Lane full before the drain starts: one overflow drop (reported) and
    // one droppable drop.
    sender.push(inbound(delete(3)));
    sender.push(inbound(delete(4)));
    sender.push(inbound(typing()));
    let drained = tokio::spawn(drain(
        receiver,
        app.workers(crate::worker::Shutdown::default()),
    ));
    until("Err run", || recording.seen().len() == 2).await;
    // One at a time, so the one-slot lane never overflows again.
    for (id, reports) in [(5, 2), (7, 3)] {
        let before = runs.load(Ordering::SeqCst);
        sender.push(inbound(delete(id)));
        until("run", || runs.load(Ordering::SeqCst) == before + 1).await;
        until("report", || recording.seen().len() == reports).await;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    drop(sender);
    tokio::time::timeout(Duration::from_secs(10), drained)
        .await
        .unwrap()
        .unwrap();
    let kinds: Vec<_> = recording.seen().into_iter().map(|s| s.kind).collect();
    assert_eq!(kinds, ["dropped", "error:x", "panic:boom"]);

    let status = ShardStatus {
        app: Arc::from("default"),
        total: 2,
    };
    status.init();
    status.update(&noctis_gateway::ShardEvent::Ready { shard: 1 });
    status.update(&noctis_gateway::ShardEvent::Ready { shard: 7 });

    let snapshot = snapshotter.snapshot().into_vec();
    let seen = check_snapshot(&snapshot, 2);
    // The HTTP edge rows are recorded end to end in the testkit's
    // `observability.rs` (Story 5.9 Matrix 49); the auto-defer and ack
    // latency rows in the pipeline tests (`int_01_auto_defer_at_deadline`).
    let all: HashSet<_> = METRICS
        .iter()
        .map(|m| m.name.to_owned())
        .filter(|name| {
            !name.starts_with("noctis_http_")
                && !name.starts_with("noctis_interaction_")
                && name != "noctis_autodefer_total"
                && name != "noctis_ack_latency_seconds"
        })
        .collect();
    assert_eq!(seen, all, "{snapshot:?}");
}

/// The value of `noctis_shard_status{shard}` in `snapshot`.
fn status_of(
    snapshot: &[(
        CompositeKey,
        Option<Unit>,
        Option<metrics::SharedString>,
        DebugValue,
    )],
    shard: &str,
) -> Option<f64> {
    snapshot.iter().find_map(|(key, .., value)| {
        let matches = key.key().name() == "noctis_shard_status"
            && key
                .key()
                .labels()
                .any(|l| l.key() == "shard" && l.value() == shard);
        match value {
            DebugValue::Gauge(v) if matches => Some(v.0),
            _ => None,
        }
    })
}

/// Matrix 30 and 31 (core level): the status gauge follows shard events.
///
/// A debugging snapshot swaps every gauge it reads to 0, so each step reads
/// only the shards its events name; `state` keeps the full picture.
#[test]
fn shard_status_gauges() {
    use noctis_gateway::ShardEvent;

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        let status = ShardStatus {
            app: Arc::from("default"),
            total: 2,
        };
        let mut state = [Some(0.0), Some(0.0)];
        let mut step = |events: &[ShardEvent]| {
            for event in events {
                status.update(event);
            }
            let snapshot = snapshotter.snapshot().into_vec();
            assert_eq!(status_of(&snapshot, "7"), None, "unknown shard 7");
            for event in events {
                let (ShardEvent::Ready { shard }
                | ShardEvent::Resumed { shard }
                | ShardEvent::Disconnected { shard, .. }
                | ShardEvent::Ended { shard, .. }) = *event
                else {
                    continue;
                };
                if let Some(slot) = state.get_mut(shard as usize) {
                    *slot = status_of(&snapshot, &shard.to_string());
                }
            }
            state
        };
        status.init();
        let snapshot = snapshotter.snapshot().into_vec();
        assert_eq!(
            (status_of(&snapshot, "0"), status_of(&snapshot, "1")),
            (Some(0.0), Some(0.0)),
            "start"
        );
        let ready = |shard| ShardEvent::Ready { shard };
        assert_eq!(step(&[ready(0), ready(1)]), [Some(1.0), Some(1.0)]);
        let disconnect = ShardEvent::Disconnected {
            shard: 1,
            close_code: Some(4000),
        };
        assert_eq!(step(&[disconnect]), [Some(1.0), Some(0.0)]);
        assert_eq!(
            step(&[ShardEvent::Resumed { shard: 1 }]),
            [Some(1.0), Some(1.0)]
        );
        let unknown = [
            ready(7),
            ShardEvent::Ended {
                shard: 7,
                close_code: None,
            },
        ];
        assert_eq!(step(&unknown), [Some(1.0), Some(1.0)]);
        let ended = ShardEvent::Ended {
            shard: 0,
            close_code: Some(4011),
        };
        assert_eq!(step(&[ended]), [Some(0.0), Some(1.0)], "Matrix 31");
    });
}

/// Dropping the `ShardStatus` (App stopped or aborted) marks every shard down.
#[test]
fn shard_status_drop_marks_down() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        let status = ShardStatus {
            app: Arc::from("default"),
            total: 2,
        };
        status.update(&noctis_gateway::ShardEvent::Ready { shard: 0 });
        status.update(&noctis_gateway::ShardEvent::Ready { shard: 1 });
        drop(status);
        let snapshot = snapshotter.snapshot().into_vec();
        assert_eq!(
            (status_of(&snapshot, "0"), status_of(&snapshot, "1")),
            (Some(0.0), Some(0.0))
        );
    });
}

/// Matrix 35: descriptions and units.
#[test]
fn describe_registers_every_row() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        describe_metrics();
        for info in METRICS {
            match info.metric_type {
                MetricType::Counter => {
                    metrics::counter!(info.name, "app" => "default").increment(1);
                }
                MetricType::Gauge => metrics::gauge!(info.name, "app" => "default").set(1.0),
                MetricType::Histogram => {
                    metrics::histogram!(info.name, "app" => "default").record(1.0);
                }
            }
        }
    });
    let snapshot = snapshotter.snapshot().into_vec();
    for info in METRICS {
        let (_, unit, description, _) = snapshot
            .iter()
            .find(|(key, ..)| key.key().name() == info.name)
            .unwrap_or_else(|| panic!("{} missing", info.name));
        assert_eq!(
            description.as_deref(),
            Some(info.description),
            "{}",
            info.name
        );
        let expected = if info.name.ends_with("_seconds") {
            Some(Unit::Seconds)
        } else if info.name.ends_with("_total") {
            Some(Unit::Count)
        } else {
            None
        };
        assert_eq!(*unit, expected, "{}", info.name);
    }
}
