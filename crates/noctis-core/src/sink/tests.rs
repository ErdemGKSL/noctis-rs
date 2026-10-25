//! Story 5.7 Matrix 1-6, 10-12, 17 (unit level), 18 and 19: Error Sinks.

use std::error::Error;

use super::*;
use crate::test_support::{Capture, Recording, Seen};
use crate::{App, AppBuilder, Plugin, Runtime};

/// An `Error` with an optional source.
#[derive(Debug)]
struct Chained {
    message: String,
    source: Option<Box<Chained>>,
}

impl fmt::Display for Chained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for Chained {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|s| s as &(dyn Error + 'static))
    }
}

/// A chain of `messages`, outermost first.
fn chain(messages: &[&str]) -> HandlerError {
    let mut error = None;
    for message in messages.iter().rev() {
        error = Some(Chained {
            message: (*message).to_owned(),
            source: error.map(Box::new),
        });
    }
    HandlerError::new(error.unwrap())
}

fn report(origin: ErrorOrigin, kind: ReportKind<'_>) -> ErrorReport<'_> {
    ErrorReport {
        origin,
        kind,
        app: "default",
        shard: Some(3),
        guild_id: Some(Id::new(41)),
        module: Some("audit"),
        handler: Some("bot::h"),
        event_type: Some("MESSAGE_DELETE"),
        extensions: None,
        middleware: None,
    }
}

/// Logs what `f` emits at TRACE level.
fn logged(f: impl FnOnce()) -> Capture {
    let capture = Capture::default();
    tracing::subscriber::with_default(capture.subscriber(), f);
    capture
}

/// Builds an App from `configure` inside a current-thread runtime.
fn build(configure: impl FnOnce(&mut AppBuilder)) -> App {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut builder = App::builder();
        builder.runtime(&Runtime::for_test());
        configure(&mut builder);
        builder.build().unwrap()
    })
}

#[test]
fn origin_names() {
    let names: Vec<_> = ErrorOrigin::ALL.iter().map(|o| o.as_str()).collect();
    assert_eq!(
        names,
        [
            "command",
            "component",
            "autocomplete",
            "event",
            "render",
            "hook"
        ]
    );
    for origin in ErrorOrigin::ALL {
        assert_eq!(origin.to_string(), origin.as_str());
        assert_eq!(ErrorOrigin::ALL[origin.index()], origin);
    }
}

/// Matrix 1: no `error_sink` call routes everything to `LogSink`.
#[test]
fn defaults_are_log_sink() {
    let app = build(|_| {});
    assert!(app.sinks().custom_origins().is_empty());
    let error = chain(&["disk full"]);
    let capture = logged(|| {
        for origin in ErrorOrigin::ALL {
            app.sinks()
                .report(report(origin, ReportKind::Error(&error)));
        }
    });
    for origin in ErrorOrigin::ALL {
        let message = format!("{origin} handler failed");
        assert_eq!(capture.lines_with(&[&message]).len(), 1, "{origin}");
    }
}

/// Matrix 2 and 3: one override, the last call wins, the rest log.
#[test]
fn override_and_last_wins() {
    let first = Arc::new(Recording::default());
    let second = Arc::new(Recording::default());
    let app = build(|b| {
        b.error_sink(ErrorOrigin::Event, Arc::clone(&first))
            .error_sink(ErrorOrigin::Event, Arc::clone(&second));
    });
    assert_eq!(app.sinks().custom_origins(), [ErrorOrigin::Event]);
    let capture = logged(|| {
        for origin in ErrorOrigin::ALL {
            app.sinks()
                .report(report(origin, ReportKind::Panic("boom")));
        }
    });
    assert!(first.seen().is_empty());
    let seen = second.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].origin, ErrorOrigin::Event);
    assert!(capture.lines_with(&["event handler panicked"]).is_empty());
    assert_eq!(capture.lines_with(&["handler panicked"]).len(), 5);
}

/// Matrix 4: one `Arc` serves every origin; each report arrives once.
#[test]
fn shared_sink() {
    let shared = Arc::new(Recording::default());
    let mut sinks = ErrorSinks::default();
    for origin in ErrorOrigin::ALL {
        sinks.set(origin, Arc::new(Arc::clone(&shared)));
    }
    for origin in ErrorOrigin::ALL {
        sinks.report(report(origin, ReportKind::Dropped));
    }
    let origins: Vec<_> = shared.seen().iter().map(|s| s.origin).collect();
    assert_eq!(origins, ErrorOrigin::ALL);
}

/// Matrix 5: distinct sinks each get exactly their origin.
#[test]
fn routing_table() {
    let recorders: Vec<_> = ErrorOrigin::ALL
        .iter()
        .map(|_| Arc::new(Recording::default()))
        .collect();
    let mut sinks = ErrorSinks::default();
    for (origin, recorder) in ErrorOrigin::ALL.iter().zip(&recorders) {
        sinks.set(*origin, Arc::clone(recorder) as Arc<dyn ErrorSink>);
    }
    for origin in ErrorOrigin::ALL {
        sinks.report(report(origin, ReportKind::Dropped));
    }
    for (origin, recorder) in ErrorOrigin::ALL.iter().zip(&recorders) {
        let seen = recorder.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].origin, *origin);
    }
}

/// Matrix 6: `build` resets the sinks.
#[test]
fn build_resets_sinks() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .error_sink(ErrorOrigin::Render, LogSink);
        assert!(format!("{builder:?}").contains("error_sinks: {Render}"));
        let first = builder.build().unwrap();
        assert_eq!(first.sinks().custom_origins(), [ErrorOrigin::Render]);
        assert!(format!("{builder:?}").contains("error_sinks: {}"));
        let second = builder.build().unwrap();
        assert!(second.sinks().custom_origins().is_empty());
    });
}

struct SinkPlugin(Arc<Recording>);

impl Plugin for SinkPlugin {
    fn install(&self, app: &mut AppBuilder) {
        app.error_sink(ErrorOrigin::Event, Arc::clone(&self.0));
    }
}

/// Matrix 7: a plugin may set a sink in `install`.
#[test]
fn plugin_sets_sink() {
    let recording = Arc::new(Recording::default());
    let app = build(|b| {
        b.plugin(SinkPlugin(Arc::clone(&recording)));
    });
    app.sinks()
        .report(report(ErrorOrigin::Event, ReportKind::Dropped));
    assert_eq!(recording.seen().len(), 1);
}

/// Matrix 10: the source chain is joined with `": "`.
#[test]
fn source_chain() {
    let error = chain(&["outer", "inner", "root"]);
    let capture = logged(|| LogSink.report(report(ErrorOrigin::Event, ReportKind::Error(&error))));
    let lines = capture.lines_with(&["event handler failed"]);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("error=outer: inner: root"),
        "{}",
        lines[0]
    );
}

/// Matrix 11: at most 16 links.
#[test]
fn chain_cap() {
    let names: Vec<String> = (0..20).map(|n| format!("e{n}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let error = chain(&refs);
    let text = Chain(error.as_error()).to_string();
    assert_eq!(text, refs[..16].join(": "));
}

/// Matrix 12: Unicode and empty messages are logged verbatim.
#[test]
fn unicode_and_empty() {
    for (message, needle) in [("💥 ünï", "error=💥 ünï"), ("", "error=")] {
        let error = chain(&[message]);
        let capture =
            logged(|| LogSink.report(report(ErrorOrigin::Event, ReportKind::Error(&error))));
        let lines = capture.lines_with(&["event handler failed"]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(needle), "{}", lines[0]);
    }
}

/// Panics on every report.
struct Panicking(AtomicUsize);

impl ErrorSink for Panicking {
    fn report(&self, _: ErrorReport<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("sink broke");
    }
}

use std::sync::atomic::{AtomicUsize, Ordering};

/// Matrix 17 (unit): a panicking sink is caught, logged and called again.
#[test]
fn panicking_sink_is_caught() {
    let sink = Arc::new(Panicking(AtomicUsize::new(0)));
    let mut sinks = ErrorSinks::default();
    sinks.set(ErrorOrigin::Event, Arc::clone(&sink) as Arc<dyn ErrorSink>);
    let capture = logged(|| {
        sinks.report(report(ErrorOrigin::Event, ReportKind::Dropped));
        sinks.report(report(ErrorOrigin::Event, ReportKind::Dropped));
    });
    assert_eq!(sink.0.load(Ordering::SeqCst), 2);
    let lines = capture.lines_with(&["error sink panicked"]);
    assert_eq!(lines.len(), 2, "{}", capture.text());
    for line in lines {
        for needle in [
            "ERROR",
            "noctis::error_sink",
            "origin=\"event\"",
            "panic=\"sink broke\"",
        ] {
            assert!(line.contains(needle), "{needle} missing from {line}");
        }
    }
}

/// Matrix 18: `LogSink` targets and messages for every origin.
#[test]
fn log_sink_origins() {
    let error = chain(&["bad"]);
    for origin in ErrorOrigin::ALL {
        let capture = logged(|| {
            LogSink.report(report(origin, ReportKind::Error(&error)));
            LogSink.report(report(origin, ReportKind::Panic("boom")));
        });
        let target = format!("noctis::{origin}:");
        let failed = format!("{origin} handler failed");
        let panicked = format!("{origin} handler panicked");
        let lines = capture.lines_with(&["ERROR", &target]);
        assert_eq!(lines.len(), 2, "{}", capture.text());
        assert!(lines[0].contains(&failed), "{}", lines[0]);
        assert!(lines[0].contains("error=bad"), "{}", lines[0]);
        assert!(lines[1].contains(&panicked), "{}", lines[1]);
        assert!(lines[1].contains("panic=\"boom\""), "{}", lines[1]);
        for line in &lines {
            for needle in [
                "app=\"default\"",
                "shard=3",
                "guild_id=41",
                "module=\"audit\"",
                "handler=\"bot::h\"",
                "event_type=\"MESSAGE_DELETE\"",
            ] {
                assert!(line.contains(needle), "{needle} missing from {line}");
            }
        }
    }
}

/// Matrix 19: a `Dropped` report without guild, module or handler.
#[test]
fn log_sink_dropped() {
    let capture = logged(|| {
        LogSink.report(ErrorReport {
            origin: ErrorOrigin::Event,
            kind: ReportKind::Dropped,
            app: "default",
            shard: Some(2),
            guild_id: None,
            module: None,
            handler: None,
            event_type: Some("GUILD_CREATE"),
            extensions: None,
            middleware: None,
        });
    });
    let lines = capture.lines_with(&["event overflow is full; event dropped"]);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    for needle in [
        "ERROR",
        "noctis::event",
        "event_type=\"GUILD_CREATE\"",
        "shard=2",
    ] {
        assert!(line.contains(needle), "{needle} missing from {line}");
    }
    for absent in ["guild_id", "module", "handler="] {
        assert!(!line.contains(absent), "{absent} in {line}");
    }
}

/// Core's drop observer reports `Event`/`Dropped` with the event's
/// context, and a panicking sink is caught there (Matrix 24).
#[test]
fn drop_observer_reports() {
    use noctis_gateway::DropObserver;

    let recording = Arc::new(Recording::default());
    let mut sinks = ErrorSinks::default();
    sinks.set(
        ErrorOrigin::Event,
        Arc::clone(&recording) as Arc<dyn ErrorSink>,
    );
    let observer = EventDropObserver {
        app: Arc::from("default"),
        sinks,
    };
    let event = crate::test_support::message_delete(5, Some(77));
    observer.overflow_dropped(2, &event);
    assert_eq!(
        recording.seen(),
        [Seen {
            origin: ErrorOrigin::Event,
            kind: "dropped".to_owned(),
            app: "default".to_owned(),
            shard: Some(2),
            guild_id: Some(77),
            module: None,
            handler: None,
            event_type: Some("MESSAGE_DELETE".to_owned()),
        }]
    );

    let mut sinks = ErrorSinks::default();
    sinks.set(
        ErrorOrigin::Event,
        Arc::new(Panicking(AtomicUsize::new(0))) as Arc<dyn ErrorSink>,
    );
    let observer = EventDropObserver {
        app: Arc::from("default"),
        sinks,
    };
    let capture = logged(|| observer.overflow_dropped(2, &event));
    assert_eq!(capture.lines_with(&["error sink panicked"]).len(), 1);
}

/// Matrix 24: a panicking observer sink never stops the pushing task.
#[test]
fn panicking_observer_keeps_lanes_running() {
    use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneConfig, lanes};
    use twilight_model::gateway::ShardId;

    let mut sinks = ErrorSinks::default();
    sinks.set(
        ErrorOrigin::Event,
        Arc::new(Panicking(AtomicUsize::new(0))) as Arc<dyn ErrorSink>,
    );
    let config = LaneConfig {
        event_capacity: std::num::NonZeroUsize::MIN,
        overflow_capacity: 0,
        drop_observer: Some(Arc::new(EventDropObserver {
            app: Arc::from("default"),
            sinks,
        })),
        ..LaneConfig::default()
    };
    let capture = logged(|| {
        let (sender, mut receiver) = lanes(config);
        for n in 1..=3 {
            sender.push(Inbound {
                shard: ShardId::ONE,
                received_at: tokio::time::Instant::now(),
                kind: InboundKind::Event(Box::new(crate::test_support::message_delete(n, None))),
            });
        }
        assert!(receiver.try_recv().is_some());
        sender.push(Inbound {
            shard: ShardId::ONE,
            received_at: tokio::time::Instant::now(),
            kind: InboundKind::Event(Box::new(crate::test_support::message_delete(4, None))),
        });
        assert_eq!(receiver.depths().event, 1);
    });
    assert_eq!(capture.lines_with(&["error sink panicked"]).len(), 2);
}

#[test]
fn error_sinks_debug_lists_custom_origins() {
    let mut sinks = ErrorSinks::default();
    assert_eq!(format!("{sinks:?}"), "{}");
    sinks.set(ErrorOrigin::Render, Arc::new(LogSink));
    sinks.set(ErrorOrigin::Command, Arc::new(LogSink));
    assert_eq!(format!("{sinks:?}"), "{Command, Render}");
}

/// Story 5.8 Matrix 27: `LogSink` logs `Cancelled` at `error` for every
/// origin, without an `error` or `panic` field.
#[test]
fn log_sink_cancelled() {
    for origin in ErrorOrigin::ALL {
        let capture = logged(|| LogSink.report(report(origin, ReportKind::Cancelled)));
        let target = format!("noctis::{origin}:");
        let message = format!("{origin} handler cancelled at shutdown");
        let lines = capture.lines_with(&["ERROR", &target, &message]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        let line = &lines[0];
        for needle in [
            "app=\"default\"",
            "shard=3",
            "guild_id=41",
            "module=\"audit\"",
        ] {
            assert!(line.contains(needle), "{needle} missing from {line}");
        }
        for absent in ["error=", "panic="] {
            assert!(!line.contains(absent), "{absent} in {line}");
        }
    }
}

/// Middleware Matrix 43: `LogSink` logs the failing Middleware and never an
/// Extension value.
#[test]
fn log_sink_names_the_middleware_never_extensions() {
    #[derive(Debug)]
    #[expect(dead_code, reason = "only its Debug must never be logged")]
    struct Secret {
        secret: &'static str,
    }
    let extensions = crate::Extensions::of(Secret { secret: "hunter2" });
    let error = HandlerError::new(fmt::Error);
    let capture = logged(|| {
        LogSink.report(ErrorReport {
            extensions: Some(&extensions),
            middleware: Some("bot::load"),
            ..report(ErrorOrigin::Command, ReportKind::Error(&error))
        });
        LogSink.report(report(ErrorOrigin::Command, ReportKind::Panic("boom")));
    });
    let text = capture.text();
    assert_eq!(
        capture
            .lines_with(&["command handler failed", "middleware=\"bot::load\""])
            .len(),
        1,
        "{text}"
    );
    assert_eq!(
        capture
            .lines_with(&["command handler panicked", "middleware="])
            .len(),
        0,
        "{text}"
    );
    assert!(
        !text.contains("hunter2") && !text.contains("Secret"),
        "{text}"
    );
}
