//! The component pipeline (Story 7.1; AD-19, AD-25, CID-04, CID-06): a
//! `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction runs the handler of
//! its Route on the decoded struct.
//!
//! 1. read the `custom_id` (and, for a click, the component type) from the
//!    interaction; a modal submit is parsed as a [`ModalSubmission`];
//! 2. split it once with [`noctis_codec::decode`] and look its Route up in
//!    the Registry's Route map with the borrowed id; check the family (a
//!    click needs a component handler of its component kind, a submit a
//!    modal handler); decode the struct (a click with no keys, a submit
//!    with `ModalForm::from_submission`);
//! 3. build the [`ComponentCtx`] or [`ModalSubmitCtx`];
//! 4. run the handler under `catch_unwind`, raced against the shutdown
//!    cancel token, recording `noctis_handler_duration_seconds{kind =
//!    "component"}`;
//! 5. a user condition is logged at `debug`; any other error, a panic or a
//!    cancellation is counted and reported to the Component Error Sink.
//!
//! Every input that does not reach a handler is a [`Rejection`]: one
//! `warn` on target `noctis::dispatch`, no handler run, no request and no
//! panic. Nothing is stored per message or component: the one task per
//! interaction ends when the handler ends. Responses, acks and the CID-06
//! notice arrive with Story 7.2.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use futures_util::FutureExt;
use noctis_codec::{DecodeError, Decoded};
use noctis_gateway::{Inbound, InboundKind};
use noctis_rules::MessageKey;
use noctis_ui::ModalSubmission;
use tokio_util::sync::CancellationToken;
use tracing::field::Empty;
use tracing::{Instrument, Span};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::ComponentType;
use twilight_model::id::Id;

use super::{INTERACTION_CREATE, Outcome, report_logged};
use crate::command::CtxInner;
use crate::component::{
    ComponentCtx, ComponentHandler, ComponentKind, HandlerFuture, ModalHandler, ModalSubmitCtx,
};
use crate::handler::AppShared;
use crate::registry::{Registry, RouteEntry, RouteHandler};
use crate::responder::Ephemerality;
use crate::sink::{ErrorOrigin, ErrorReport, ReportKind};
use crate::worker::panic_message;

/// Why a component or modal interaction did not reach a handler; its
/// [`as_str`](Self::as_str) is the `reason` of the one `warn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rejection {
    /// The `custom_id` failed codec phase 1, the click has no data or the
    /// modal submission is unparsable.
    Malformed,
    /// No handler has the Route id, or only one of the other family.
    UnknownRoute,
    /// The clicked component's type is not the handler's kind.
    KindMismatch,
    /// The id is from another shape or schema of the Route (Story 7.3
    /// routes it).
    Stale,
    /// A bad, missing or unverifiable integrity tag (signed Routes until
    /// Story 7.2 loads `NOCTIS_PAYLOAD_KEY`).
    Forged,
    /// A Ref body (Story 7.5 resolves it).
    RefUnresolved,
    /// The inline body does not decode into the struct.
    Undecodable,
    /// The modal inputs do not decode into the struct.
    SubmitInvalid,
    /// The interaction has neither `member` nor `user`.
    NoAuthor,
}

impl Rejection {
    /// The log label.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnknownRoute => "unknown_route",
            Self::KindMismatch => "kind_mismatch",
            Self::Stale => "stale",
            Self::Forged => "forged",
            Self::RefUnresolved => "ref_unresolved",
            Self::Undecodable => "undecodable",
            Self::SubmitInvalid => "submit_invalid",
            Self::NoAuthor => "no_author",
        }
    }

    /// The rejection of a phase-2 decode error.
    pub(crate) const fn from_decode(error: &DecodeError) -> Self {
        match error {
            DecodeError::Stale(_) => Self::Stale,
            DecodeError::Forged(_) => Self::Forged,
            _ => Self::Undecodable,
        }
    }
}

/// The component kind of a clicked component's type, if it has one.
pub(crate) const fn kind_of(kind: ComponentType) -> Option<ComponentKind> {
    match kind {
        ComponentType::Button => Some(ComponentKind::Button),
        ComponentType::TextSelectMenu => Some(ComponentKind::StringSelect),
        ComponentType::UserSelectMenu => Some(ComponentKind::UserSelect),
        ComponentType::RoleSelectMenu => Some(ComponentKind::RoleSelect),
        ComponentType::MentionableSelectMenu => Some(ComponentKind::MentionableSelect),
        ComponentType::ChannelSelectMenu => Some(ComponentKind::ChannelSelect),
        _ => None,
    }
}

/// What the interaction carries.
enum Input {
    /// A click: the `custom_id` and the component kind of its type.
    Component {
        custom_id: String,
        kind: Option<ComponentKind>,
    },
    /// A modal submit.
    Modal(Box<ModalSubmission>),
}

impl Input {
    fn custom_id(&self) -> &str {
        match self {
            Self::Component { custom_id, .. } => custom_id,
            Self::Modal(submission) => submission.custom_id(),
        }
    }
}

/// The handler an input resolved to, with what its start needs.
enum Target<'a> {
    Component(&'a ComponentHandler),
    Modal(&'a ModalHandler, &'a ModalSubmission),
}

impl Target<'_> {
    fn start(&self, parts: CtxInner, decoded: &Decoded<'_>) -> Result<HandlerFuture, Rejection> {
        match self {
            Self::Component(handler) => handler.start(ComponentCtx::from_parts(parts), decoded),
            Self::Modal(handler, submission) => {
                handler.start(ModalSubmitCtx::from_parts(parts), decoded, submission)
            }
        }
    }
}

/// The family of an interaction: a click (with the component kind of its
/// type, if any) or a modal submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Component(Option<ComponentKind>),
    Modal,
}

/// Resolves the handler of `decoded` for an interaction of `family`
/// (stage 2 without the struct decode).
pub(crate) fn resolve<'r>(
    registry: &'r Registry,
    decoded: &Decoded<'_>,
    family: Family,
) -> Result<&'r Arc<RouteEntry>, Rejection> {
    let entry = registry
        .route(decoded.route())
        .ok_or(Rejection::UnknownRoute)?;
    match (&entry.handler, family) {
        (RouteHandler::Component(handler), Family::Component(kind)) => {
            if kind == Some(handler.kind()) {
                Ok(entry)
            } else {
                Err(Rejection::KindMismatch)
            }
        }
        (RouteHandler::Modal(_), Family::Modal) => Ok(entry),
        _ => Err(Rejection::UnknownRoute),
    }
}

/// Logs `rejection` (one `warn`, inside `span`).
fn reject(span: &Span, shard: u32, route: Option<&str>, rejection: Rejection) {
    span.in_scope(|| {
        tracing::warn!(
            target: "noctis::dispatch",
            reason = rejection.as_str(),
            shard,
            route,
            "component interaction rejected"
        );
    });
}

/// Reads the input of a click or a modal submit; `None` for any other
/// interaction type.
fn read_input(
    kind: InteractionType,
    data: Option<InteractionData>,
    raw: &[u8],
) -> Option<Result<Input, Rejection>> {
    match kind {
        InteractionType::MessageComponent => Some(match data {
            Some(InteractionData::MessageComponent(data)) => {
                let data = *data;
                Ok(Input::Component {
                    kind: kind_of(data.component_type),
                    custom_id: data.custom_id,
                })
            }
            _ => Err(Rejection::Malformed),
        }),
        InteractionType::ModalSubmit => Some(
            ModalSubmission::from_slice(raw)
                .map(|s| Input::Modal(Box::new(s)))
                .map_err(|_| Rejection::Malformed),
        ),
        _ => None,
    }
}

/// Runs one `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction through the
/// component pipeline (see the module docs) in span `noctis.interaction`.
pub(crate) async fn run_component(
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    inbound: Inbound,
    cancel: CancellationToken,
) {
    let shard = inbound.shard.number();
    let InboundKind::Interaction(interaction, raw) = inbound.kind else {
        return;
    };
    let span = tracing::info_span!(
        "noctis.interaction",
        app = &*app.app,
        shard,
        guild_id = interaction.guild_id.map(Id::get),
        module = Empty,
        route = Empty,
    );
    let author = interaction.author_id();
    let Interaction {
        kind,
        data,
        locale,
        guild_locale,
        guild_id,
        channel,
        ..
    } = *interaction;
    // Stage 1: read the input.
    let input = match read_input(kind, data, raw.as_bytes()) {
        None => return,
        Some(Ok(input)) => input,
        Some(Err(rejection)) => return reject(&span, shard, None, rejection),
    };
    // Stage 2: look up and decode.
    let Ok(decoded) = noctis_codec::decode(input.custom_id()) else {
        return reject(&span, shard, None, Rejection::Malformed);
    };
    let route = decoded.route();
    let family = match &input {
        Input::Component { kind, .. } => Family::Component(*kind),
        Input::Modal(_) => Family::Modal,
    };
    let entry = match resolve(&registry, &decoded, family) {
        Ok(entry) => entry,
        Err(rejection) => return reject(&span, shard, Some(route), rejection),
    };
    let target = match (&entry.handler, &input) {
        (RouteHandler::Component(handler), Input::Component { .. }) => Target::Component(handler),
        (RouteHandler::Modal(handler), Input::Modal(submission)) => {
            Target::Modal(handler, submission)
        }
        _ => return reject(&span, shard, Some(route), Rejection::UnknownRoute),
    };
    span.record("module", entry.module);
    span.record("route", route);
    let Some(user_id) = author else {
        return reject(&span, shard, Some(route), Rejection::NoAuthor);
    };
    // Stage 3: the context.
    let parts = CtxInner {
        module: entry.module,
        shard,
        guild_id,
        channel_id: channel.as_ref().map(|c| c.id),
        user_id,
        user_locale: locale,
        guild_locale,
        app: Some(Arc::clone(&app)),
        ..CtxInner::detached_parts(Ephemerality::Public)
    };
    let report = Report {
        app: &app,
        shard,
        guild_id,
        module: entry.module,
        handler: entry.handler.name(),
    };
    // Stage 4: decode the struct and run the handler.
    let started = if cancel.is_cancelled() {
        None
    } else {
        let started = span.in_scope(|| {
            std::panic::catch_unwind(AssertUnwindSafe(|| target.start(parts, &decoded)))
        });
        match started {
            Ok(Err(rejection)) => return reject(&span, shard, Some(route), rejection),
            Ok(Ok(fut)) => Some(Ok(fut)),
            Err(panic) => Some(Err(panic)),
        }
    };
    let outcome = match started {
        None => Outcome::Cancelled,
        Some(started) => execute(&span, &cancel, started, &app).await,
    };
    // Stage 5: report a failure.
    finish(&span, &report, &outcome);
}

/// Where a component handler run is reported.
struct Report<'a> {
    app: &'a Arc<AppShared>,
    shard: u32,
    guild_id: Option<Id<twilight_model::id::marker::GuildMarker>>,
    module: &'static str,
    handler: &'static str,
}

/// Runs the started handler under `catch_unwind`, raced against `cancel`,
/// and records its duration.
async fn execute(
    span: &Span,
    cancel: &CancellationToken,
    started: Result<HandlerFuture, Box<dyn std::any::Any + Send>>,
    app: &AppShared,
) -> Outcome {
    let started_at = Instant::now();
    let outcome = match started {
        Ok(fut) => {
            let run = AssertUnwindSafe(async move { fut.await.map(|_| ()) })
                .catch_unwind()
                .instrument(span.clone());
            tokio::select! {
                biased;
                () = cancel.cancelled() => Outcome::Cancelled,
                result = run => match result {
                    Ok(result) => Outcome::Returned(result),
                    Err(panic) => Outcome::Panicked(panic),
                },
            }
        }
        Err(panic) => Outcome::Panicked(panic),
    };
    metrics::histogram!(
        "noctis_handler_duration_seconds",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Component.as_str()
    )
    .record(started_at.elapsed().as_secs_f64());
    outcome
}

/// Stage 5: logs a user condition at `debug`; counts and reports any other
/// failure to the Component Error Sink.
fn finish(span: &Span, report: &Report<'_>, outcome: &Outcome) {
    let kind = match outcome {
        Outcome::Returned(Ok(())) => return,
        Outcome::Returned(Err(error)) if error.is_user() => {
            span.in_scope(|| {
                tracing::debug!(
                    target: "noctis::component",
                    handler = report.handler,
                    key = error.user_message().map(MessageKey::as_str),
                    "user condition"
                );
            });
            return;
        }
        Outcome::Returned(Err(error)) => ReportKind::Error(error),
        Outcome::Panicked(panic) => ReportKind::Panic(panic_message(&**panic)),
        Outcome::Cancelled => ReportKind::Cancelled,
    };
    let app = report.app;
    metrics::counter!(
        "noctis_handler_errors_total",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Component.as_str()
    )
    .increment(1);
    span.in_scope(|| {
        report_logged(
            app,
            ErrorReport {
                origin: ErrorOrigin::Component,
                kind,
                app: &app.app,
                shard: Some(report.shard),
                guild_id: report.guild_id,
                module: Some(report.module),
                handler: Some(report.handler),
                event_type: Some(INTERACTION_CREATE),
            },
        );
    });
}
