//! The edge's metrics; every label value comes from a fixed set.

use std::sync::Arc;
use std::time::Duration;

/// `noctis_http_phase_seconds{phase}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Dns,
    Connect,
    Tls,
    Ttfb,
    Total,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dns => "dns",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Ttfb => "ttfb",
            Self::Total => "total",
        }
    }
}

/// Why a connection was opened (`noctis_http_connections_total{reason}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    /// The keeper's first connection.
    Warmup,
    /// The keeper replaced a closed or suspect connection.
    Redial,
    /// A hedged (or retried) callback attempt on a never-pooled connection.
    Hedge,
    /// A request found no ready connection.
    Demand,
}

impl Reason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Redial => "redial",
            Self::Hedge => "hedge",
            Self::Demand => "demand",
        }
    }
}

/// `noctis_interaction_ack_hedges_total{outcome}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HedgeOutcome {
    Won,
    Lost,
    AlreadyAcknowledged,
}

/// The `status_class` label of a status, or `"error"` without one.
pub(crate) fn status_class(status: Option<u16>) -> &'static str {
    match status {
        Some(100..=199) => "1xx",
        Some(200..=299) => "2xx",
        Some(300..=399) => "3xx",
        Some(400..=499) => "4xx",
        Some(500..=599) => "5xx",
        _ => "error",
    }
}

/// Emits the edge metrics with the App's `app` label.
#[derive(Debug, Clone)]
pub(crate) struct EdgeMetrics {
    app: Arc<str>,
}

impl EdgeMetrics {
    pub(crate) fn new(app: Arc<str>) -> Self {
        Self { app }
    }

    /// The `app` label value.
    pub(crate) fn app(&self) -> &str {
        &self.app
    }

    pub(crate) fn request(&self, status: Option<u16>) {
        metrics::counter!(
            "noctis_http_requests_total",
            "app" => Arc::clone(&self.app),
            "status_class" => status_class(status)
        )
        .increment(1);
    }

    pub(crate) fn phase(&self, phase: Phase, elapsed: Duration) {
        metrics::histogram!(
            "noctis_http_phase_seconds",
            "app" => Arc::clone(&self.app),
            "phase" => phase.as_str()
        )
        .record(elapsed.as_secs_f64());
    }

    pub(crate) fn connection(&self, reason: Reason) {
        metrics::counter!(
            "noctis_http_connections_total",
            "app" => Arc::clone(&self.app),
            "reason" => reason.as_str()
        )
        .increment(1);
    }

    pub(crate) fn reused(&self, reused: bool) {
        metrics::counter!(
            "noctis_http_conn_reused_total",
            "app" => Arc::clone(&self.app),
            "reused" => if reused { "true" } else { "false" }
        )
        .increment(1);
    }

    pub(crate) fn ping_rtt(&self, rtt: Duration) {
        metrics::histogram!("noctis_http_h2_ping_rtt_seconds", "app" => Arc::clone(&self.app))
            .record(rtt.as_secs_f64());
    }

    pub(crate) fn ping_timeout(&self) {
        metrics::counter!("noctis_http_ping_timeouts_total", "app" => Arc::clone(&self.app))
            .increment(1);
    }

    pub(crate) fn hedge(&self, outcome: HedgeOutcome) {
        let outcome = match outcome {
            HedgeOutcome::Won => "won",
            HedgeOutcome::Lost => "lost",
            HedgeOutcome::AlreadyAcknowledged => "40060",
        };
        metrics::counter!(
            "noctis_interaction_ack_hedges_total",
            "app" => Arc::clone(&self.app),
            "outcome" => outcome
        )
        .increment(1);
    }
}
