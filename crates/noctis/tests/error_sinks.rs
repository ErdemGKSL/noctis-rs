//! Story 5.7 Matrix 4 and 7 through the facade: a custom sink, one `Arc`
//! sink shared by every origin, and a plugin installing a sink.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noctis::prelude::*;
use noctis::{ErrorReport, LogSink, Plugin, ReportKind, Runtime};

/// Counts reports by kind.
#[derive(Default)]
struct Counting {
    errors: AtomicUsize,
    panics: AtomicUsize,
    dropped: AtomicUsize,
}

impl ErrorSink for Counting {
    fn report(&self, report: ErrorReport<'_>) {
        let counter = match report.kind {
            ReportKind::Error(_) => &self.errors,
            ReportKind::Panic(_) => &self.panics,
            ReportKind::Dropped => &self.dropped,
            _ => return,
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

struct AlertPlugin(Arc<Counting>);

impl Plugin for AlertPlugin {
    fn install(&self, app: &mut AppBuilder) {
        app.error_sink(ErrorOrigin::Event, Arc::clone(&self.0));
    }
}

/// Story 5.8: `ReportKind::Cancelled` is reachable through the facade.
#[test]
fn cancelled_kind_through_facade() {
    let kind = ReportKind::Cancelled;
    assert!(matches!(kind, ReportKind::Cancelled));
    assert!(matches!(
        noctis::Error::from(noctis::RunError::AllShardsEnded),
        noctis::Error::Run(noctis::RunError::AllShardsEnded)
    ));
}

#[test]
fn origins_and_names() {
    let names: Vec<_> = ErrorOrigin::ALL.iter().map(ToString::to_string).collect();
    assert_eq!(
        names,
        ["command", "component", "autocomplete", "event", "render"]
    );
}

/// Matrix 4: one `Arc` registered on every origin.
#[tokio::test(flavor = "current_thread")]
async fn shared_sink_on_every_origin() {
    let shared = Arc::new(Counting::default());
    let mut builder = App::builder();
    builder.runtime(&Runtime::for_test());
    for origin in ErrorOrigin::ALL {
        builder.error_sink(origin, Arc::clone(&shared));
    }
    let debug = format!("{builder:?}");
    assert!(
        debug.contains("error_sinks: {Command, Component, Autocomplete, Event, Render}"),
        "{debug}"
    );
    let app = builder.build().unwrap();
    assert!(format!("{app:?}").contains("error_sinks: {Command, Component"));
    assert_eq!(Arc::strong_count(&shared), 6);
}

/// Matrix 7: a plugin sets the `Event` sink in `install`.
#[tokio::test(flavor = "current_thread")]
async fn plugin_installs_sink() {
    let counting = Arc::new(Counting::default());
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .error_sink(ErrorOrigin::Render, LogSink)
        .plugin(AlertPlugin(Arc::clone(&counting)));
    let app = builder.build().unwrap();
    assert!(format!("{app:?}").contains("error_sinks: {Event, Render}"));
    assert_eq!(counting.errors.load(Ordering::SeqCst), 0);
}

#[test]
fn catalog_through_the_facade() {
    let names: Vec<_> = noctis::observe::METRICS.iter().map(|m| m.name).collect();
    assert!(names.contains(&"noctis_handler_errors_total"));
    noctis::observe::describe_metrics();
}
