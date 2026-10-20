//! Error Sinks: where handler failures go (FR-22).
//!
//! [`AlertSink`] logs every `Event` and `Render` report with `tracing` under its own
//! target, so an alerting rule can match `bot::alert`. It never blocks: a real pager
//! would forward the copied fields to a channel and send from a spawned task.
//! Origins without a custom sink keep the default `LogSink`.

use std::sync::Arc;

use noctis::prelude::*;
use noctis::{ErrorReport, ReportKind};

/// Logs a short alert line per report; shared by the `Event` and `Render` origins.
#[derive(Debug, Default)]
pub struct AlertSink;

impl ErrorSink for AlertSink {
    fn report(&self, report: ErrorReport<'_>) {
        let what = match report.kind {
            ReportKind::Error(_) => "error",
            ReportKind::Panic(_) => "panic",
            ReportKind::Dropped => "dropped",
            _ => "other",
        };
        // Ids and names only: never log payloads or tokens.
        tracing::error!(
            target: "bot::alert",
            origin = %report.origin,
            what,
            shard = report.shard,
            guild_id = report.guild_id.map(noctis::twilight::model::id::Id::get),
            module = report.module,
            handler = report.handler,
            event_type = report.event_type,
            "handler failure"
        );
    }
}

/// Routes `Event` and `Render` reports to one shared [`AlertSink`].
pub fn install(builder: &mut AppBuilder) -> &mut AppBuilder {
    let alert = Arc::new(AlertSink);
    builder
        .error_sink(ErrorOrigin::Event, Arc::clone(&alert))
        .error_sink(ErrorOrigin::Render, alert)
}
