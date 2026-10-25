//! The observability contract: every metric noctis emits (NFR-5, AD-29).
//!
//! Metrics go through the [`metrics`](https://docs.rs/metrics) facade;
//! install a recorder (e.g. `metrics-exporter-prometheus`) to collect them.
//! Every name starts with `noctis_`, every metric has an `app` label, and
//! no label carries a user, guild, channel or message id or free text, so
//! cardinality stays bounded. [`METRICS`] lists them all;
//! [`describe_metrics`] registers their descriptions and units (`App::run`
//! calls it).
//!
//! Spans: `noctis.shard{app, shard}` around each shard task,
//! `noctis.event{app, shard, guild_id, module, handler, event_type}` around
//! each event handler run, and
//! `noctis.interaction{app, shard, guild_id, module, path}` around each
//! interaction (`module` and `path`, the Command Path or menu name, are
//! recorded once the Command resolves),
//! `noctis.dispatch{app, guild_id, module, path}` around each
//! `Dispatcher::run_command`,
//! `noctis.middleware{app, middleware, phase}` around each Middleware call
//! (`phase` is `before` or `after`), and
//! `noctis.http{app, kind, method, attempt, reused, status_class}` around
//! each upstream attempt of the Discord HTTP edge (`kind` is `forward`,
//! `callback` or `warmup`), and
//! `noctis.render{app, template}` around each `ctx.render` (`template` is
//! the Template id). No span field or label carries a path, a token, an id
//! or a header value.
//!
//! ```
//! use noctis_core::observe::{METRICS, MetricType};
//!
//! let status = METRICS.iter().find(|m| m.name == "noctis_shard_status").unwrap();
//! assert_eq!(status.metric_type, MetricType::Gauge);
//! assert_eq!(status.labels, ["app", "shard"]);
//! ```

/// What kind of value a metric holds.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricType {
    /// A monotonically increasing count (`_total`).
    Counter,
    /// A value that goes up and down.
    Gauge,
    /// A distribution of samples (`_seconds`).
    Histogram,
}

/// One row of the metric catalog.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricInfo {
    /// The metric name.
    pub name: &'static str,
    /// Counter, gauge or histogram.
    pub metric_type: MetricType,
    /// The label names, `app` first.
    pub labels: &'static [&'static str],
    /// What it measures.
    pub description: &'static str,
}

const fn row(
    name: &'static str,
    metric_type: MetricType,
    labels: &'static [&'static str],
    description: &'static str,
) -> MetricInfo {
    MetricInfo {
        name,
        metric_type,
        labels,
        description,
    }
}

/// Every metric noctis emits, with its type and label names.
///
/// | Metric | Type | Labels |
/// | --- | --- | --- |
/// | `noctis_lane_depth` | gauge | `app`, `lane` (`interaction`, `event`) |
/// | `noctis_event_overflow_depth` | gauge | `app` |
/// | `noctis_events_dropped_total` | counter | `app`, `event_type` |
/// | `noctis_handler_duration_seconds` | histogram | `app`, `kind` (`command`, `component`, `autocomplete`, `event`, `hook`) |
/// | `noctis_handler_errors_total` | counter | `app`, `kind` (`command`, `component`, `autocomplete`, `event`, `hook`) |
/// | `noctis_autodefer_total` | counter | `app` |
/// | `noctis_ack_latency_seconds` | histogram | `app` |
/// | `noctis_ref_lookups_total` | counter | `app`, `result` (`hit`, `miss`, `stale`, `forged`) |
/// | `noctis_rate_limited_total` | counter | `app`, `kind` (`command`, `component`) |
/// | `noctis_shard_status` | gauge | `app`, `shard` (1 up, 0 down) |
/// | `noctis_http_requests_total` | counter | `app`, `status_class` (`1xx`..`5xx`, `error`) |
/// | `noctis_http_rate_limited_total` | counter | `app`, `scope` (`user`, `global`, `shared`, `unknown`) |
/// | `noctis_http_invalid_requests_total` | counter | `app`, `status` (`401`, `403`, `429`; a `shared` 429 is not counted) |
/// | `noctis_http_phase_seconds` | histogram | `app`, `phase` (`dns`, `connect`, `tls`, `ttfb`, `total`) |
/// | `noctis_http_connections_total` | counter | `app`, `reason` (`warmup`, `redial`, `hedge`, `demand`) |
/// | `noctis_http_conn_reused_total` | counter | `app`, `reused` (`true`, `false`) |
/// | `noctis_http_h2_ping_rtt_seconds` | histogram | `app` |
/// | `noctis_http_ping_timeouts_total` | counter | `app` |
/// | `noctis_interaction_ack_hedges_total` | counter | `app`, `outcome` (`won`, `lost`, `40060`) |
/// | `noctis_render_duration_seconds` | histogram | `app`, `template` (the Template id) |
/// | `noctis_render_queue_depth` | gauge | `app` (this App's `ctx.render` calls awaiting a render; a cancelled call stops counting, and its job is skipped while queued or stops before its next frame) |
/// | `noctis_render_rejected_total` | counter | `app`, `reason` (`queue_full`, `unknown_template`, `unsupported`, `over_cap`) |
/// | `noctis_discord_cache_update_errors_total` | counter | `app` (failed or timed-out Discord cache updates; the event or interaction still runs) |
/// | `noctis_voice_calls` | gauge | `app` (live and pending voice calls; feature `voice`) |
///
/// ```
/// assert!(noctis_core::observe::METRICS.iter().all(|m| m.labels[0] == "app"));
/// ```
pub const METRICS: &[MetricInfo] = &[
    row(
        "noctis_lane_depth",
        MetricType::Gauge,
        &["app", "lane"],
        "Items queued in the Interaction Lane (lane=interaction) or the event lane (lane=event).",
    ),
    row(
        "noctis_event_overflow_depth",
        MetricType::Gauge,
        &["app"],
        "Non-droppable events waiting in the overflow behind a full event lane.",
    ),
    row(
        "noctis_events_dropped_total",
        MetricType::Counter,
        &["app", "event_type"],
        "Gateway events dropped because the event lane or its overflow was full.",
    ),
    row(
        "noctis_handler_duration_seconds",
        MetricType::Histogram,
        &["app", "kind"],
        "Handler run time in seconds, including errors and panics.",
    ),
    row(
        "noctis_handler_errors_total",
        MetricType::Counter,
        &["app", "kind"],
        "Handler runs that returned an error or panicked.",
    ),
    row(
        "noctis_autodefer_total",
        MetricType::Counter,
        &["app"],
        "Interactions the auto-defer acknowledged.",
    ),
    row(
        "noctis_ack_latency_seconds",
        MetricType::Histogram,
        &["app"],
        "Time from gateway receipt to the initial interaction callback.",
    ),
    row(
        "noctis_ref_lookups_total",
        MetricType::Counter,
        &["app", "result"],
        "Ref lookups by result: hit, miss, stale or forged.",
    ),
    row(
        "noctis_rate_limited_total",
        MetricType::Counter,
        &["app", "kind"],
        "Invocations a declared rate limit refused, by kind: command or component.",
    ),
    row(
        "noctis_shard_status",
        MetricType::Gauge,
        &["app", "shard"],
        "Shard connection status: 1 when the session is up (READY or RESUMED), 0 otherwise.",
    ),
    row(
        "noctis_http_requests_total",
        MetricType::Counter,
        &["app", "status_class"],
        "Discord HTTP upstream attempts by status class (error: no response).",
    ),
    row(
        "noctis_http_rate_limited_total",
        MetricType::Counter,
        &["app", "scope"],
        "Discord HTTP upstream 429 answers by X-RateLimit-Scope (RL-04).",
    ),
    row(
        "noctis_http_invalid_requests_total",
        MetricType::Counter,
        &["app", "status"],
        "Discord HTTP upstream 401, 403 and non-shared 429 answers, which count against \
         Discord's invalid-request limit of 10,000 per 10 minutes per IP (RL-03).",
    ),
    row(
        "noctis_http_phase_seconds",
        MetricType::Histogram,
        &["app", "phase"],
        "Discord HTTP phase durations in seconds: dns, connect and tls per new connection, \
         ttfb and total per attempt.",
    ),
    row(
        "noctis_http_connections_total",
        MetricType::Counter,
        &["app", "reason"],
        "Discord HTTP upstream connections opened, by reason (warmup, redial, hedge, demand).",
    ),
    row(
        "noctis_http_conn_reused_total",
        MetricType::Counter,
        &["app", "reused"],
        "Discord HTTP attempts on a reused (true) or new (false) connection.",
    ),
    row(
        "noctis_http_h2_ping_rtt_seconds",
        MetricType::Histogram,
        &["app"],
        "HTTP/2 PING round trips of the warm Discord connection, in seconds.",
    ),
    row(
        "noctis_http_ping_timeouts_total",
        MetricType::Counter,
        &["app"],
        "Discord HTTP/2 connections closed because a PING went unanswered.",
    ),
    row(
        "noctis_interaction_ack_hedges_total",
        MetricType::Counter,
        &["app", "outcome"],
        "Interaction callback hedges that won or lost, and callbacks Discord answered with 40060.",
    ),
    row(
        "noctis_render_duration_seconds",
        MetricType::Histogram,
        &["app", "template"],
        "Successful ctx.render time in seconds, queue wait included, by Template id.",
    ),
    row(
        "noctis_render_queue_depth",
        MetricType::Gauge,
        &["app"],
        "This App's ctx.render calls awaiting a render; a cancelled call stops counting, and its job is skipped while queued or stops before its next frame.",
    ),
    row(
        "noctis_render_rejected_total",
        MetricType::Counter,
        &["app", "reason"],
        "Renders rejected by reason: queue_full, unknown_template, unsupported or over_cap.",
    ),
    row(
        "noctis_discord_cache_update_errors_total",
        MetricType::Counter,
        &["app"],
        "Discord cache updates that failed or exceeded 1 s; the event or interaction still \
         reaches its handlers.",
    ),
    row(
        "noctis_voice_calls",
        MetricType::Gauge,
        &["app"],
        "Live and pending voice calls (feature `voice`)",
    ),
];

/// Registers the description (and unit) of every [`METRICS`] row with the
/// installed `metrics` recorder: `Seconds` for `_seconds`, `Count` for
/// `_total`, no unit otherwise. `App::run` calls it; calling it again is
/// harmless.
///
/// ```
/// noctis_core::observe::describe_metrics();
/// ```
pub fn describe_metrics() {
    use metrics::Unit;

    for info in METRICS {
        let unit = if info.name.ends_with("_seconds") {
            Some(Unit::Seconds)
        } else if info.name.ends_with("_total") {
            Some(Unit::Count)
        } else {
            None
        };
        match (info.metric_type, unit) {
            (MetricType::Counter, Some(unit)) => {
                metrics::describe_counter!(info.name, unit, info.description);
            }
            (MetricType::Counter, None) => metrics::describe_counter!(info.name, info.description),
            (MetricType::Gauge, Some(unit)) => {
                metrics::describe_gauge!(info.name, unit, info.description);
            }
            (MetricType::Gauge, None) => metrics::describe_gauge!(info.name, info.description),
            (MetricType::Histogram, Some(unit)) => {
                metrics::describe_histogram!(info.name, unit, info.description);
            }
            (MetricType::Histogram, None) => {
                metrics::describe_histogram!(info.name, info.description);
            }
        }
    }
}

/// Sets `noctis_shard_status{app, shard}` to 1 (`up`) or 0; a shard
/// `>= total` is ignored.
pub(crate) fn set_shard_status(app: &std::sync::Arc<str>, shard: u32, total: u32, up: bool) {
    if shard >= total {
        return;
    }
    metrics::gauge!(
        "noctis_shard_status",
        "app" => std::sync::Arc::clone(app),
        "shard" => shard.to_string()
    )
    .set(if up { 1.0 } else { 0.0 });
}

#[cfg(test)]
mod tests;
