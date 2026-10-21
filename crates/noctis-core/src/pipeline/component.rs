//! The component pipeline (Stories 7.1 and 7.2; AD-5, AD-19, AD-23,
//! AD-25, AD-29; CID-04, CID-06, INT-01, INT-04): a `MESSAGE_COMPONENT` or
//! `MODAL_SUBMIT` interaction runs the handler of its Route on the decoded
//! struct, with a responder over the interaction.
//!
//! 1. **Read the input:** the `custom_id` (and, for a click, the component
//!    type) from the interaction; a modal submit is parsed as a
//!    [`ModalSubmission`]; the source message (`message.id`, `channel_id`,
//!    `flags`) is read from the raw JSON.
//! 2. **Look up the Route:** split the `custom_id` once with
//!    [`noctis_codec::decode`], look its Route up in the Registry's Route
//!    map with the borrowed id and check the family (a click needs a
//!    component handler of its component kind, a submit a modal handler).
//! 3. **Build the context and the responder state:** the [`ComponentCtx`]
//!    or [`ModalSubmitCtx`] and the shared responder state with the
//!    declared `response`.
//! 4. **Decode the struct and start the handler:** the Payload is decoded
//!    once, verifying a `signed` Route's tag with the App's payload key ring
//!    (current or previous key); a modal's inputs are decoded from that
//!    verified Payload and the submission (`ModalFromPayload`).
//! 5. **Arm the auto-defer** (shared with Commands): at the ack deadline a
//!    type-6 callback under `response = update` when a source message
//!    exists, else type 5 (`EPHEMERAL` only for `response = ephemeral`); an
//!    `auto_defer = false` handler gets only the 2.9 s error log.
//! 6. **Run and answer:** run the handler under `catch_unwind`, raced
//!    against the shutdown cancel token, and send what it returned per the
//!    declared `response`; an error or a panic sends the localized
//!    ephemeral notice, then a user condition is logged at `debug`; any
//!    other error, a panic, a failed send or a cancellation is counted and
//!    reported to the Component Error Sink.
//!
//! Every input that does not reach a handler is a [`Rejection`]: one
//! `warn` on target `noctis::dispatch` and, except a stale or Ref id (Stories
//! 7.3 and 7.5 route those), the CID-06 generic ephemeral notice; no
//! handler run and no panic. Nothing is stored per message or component:
//! the one auto-defer task per interaction ends when the handler run ends.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use noctis_codec::{DecodeError, Decoded, Verify};
use noctis_gateway::{Inbound, InboundKind};
use noctis_i18n::keys;
use noctis_rules::Locale;
use noctis_ui::{ModalSubmission, SubmitError};
use tokio_util::sync::CancellationToken;
use tracing::field::Empty;
use tracing::{Instrument, Span};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::ComponentType;
use twilight_model::id::Id;

use super::{Armed, INTERACTION_CREATE, Report, Started, USER_INSTALL_FOLLOWUPS, arm, run_started};
use crate::command::CtxInner;
use crate::component::{
    ComponentCtx, ComponentHandler, ComponentKind, HandlerFuture, ModalHandler, ModalSubmitCtx,
};
use crate::handler::AppShared;
use crate::notice;
use crate::registry::{Registry, RouteEntry, RouteHandler};
use crate::responder::{
    Declared, Ephemerality, InteractionTarget, Shared, SourceMessage, Transport,
};
use crate::sink::ErrorOrigin;

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
    /// A bad or missing integrity tag, or one no key of the ring verifies.
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

    /// Whether the rejection answers with the CID-06 notice: every one but
    /// a stale or Ref id (Stories 7.3 and 7.5 route those).
    pub(crate) const fn sends_notice(self) -> bool {
        !matches!(self, Self::Stale | Self::RefUnresolved)
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
    fn start(
        &self,
        parts: CtxInner,
        decoded: &Decoded<'_>,
        shared: Arc<Shared>,
        keys: Option<&dyn Verify>,
    ) -> Result<HandlerFuture, Rejection> {
        match self {
            Self::Component(handler) => {
                handler.start(ComponentCtx::from_parts(parts), decoded, shared, keys)
            }
            Self::Modal(handler, submission) => handler.start(
                ModalSubmitCtx::from_parts(parts),
                decoded,
                submission,
                shared,
                keys,
            ),
        }
    }

    /// The handler's declared `response`.
    fn response(&self) -> Declared {
        match self {
            Self::Component(handler) => handler.response(),
            Self::Modal(handler, _) => handler.response(),
        }
    }

    /// Whether the handler is auto-deferred.
    fn auto_defer(&self) -> bool {
        match self {
            Self::Component(handler) => handler.auto_defer(),
            Self::Modal(handler, _) => handler.auto_defer(),
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
/// (stage 2: look up the Route).
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
fn log_rejection(span: &Span, shard: u32, route: Option<&str>, rejection: Rejection) {
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

/// What a rejection needs to answer: the span, the shard, the App and the
/// notice locale.
struct Rejecter<'a> {
    span: &'a Span,
    shard: u32,
    app: &'a AppShared,
    locale: Locale,
}

impl Rejecter<'_> {
    /// Logs `rejection`, then (except a stale or Ref id) sends the CID-06
    /// generic ephemeral notice over `shared`; a failed send is one `warn`.
    async fn reject(&self, route: Option<&str>, rejection: Rejection, shared: &Arc<Shared>) {
        log_rejection(self.span, self.shard, route, rejection);
        if !rejection.sends_notice() {
            return;
        }
        let text = self
            .span
            .in_scope(|| notice::text(&*self.app.strings, self.locale, keys::ERROR_GENERIC));
        if let Err(error) = shared.notice(text).instrument(self.span.clone()).await {
            self.span.in_scope(|| {
                tracing::warn!(
                    target: "noctis::dispatch",
                    reason = rejection.as_str(),
                    %error,
                    "could not send the error notice"
                );
            });
        }
    }
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
                .map_err(|e| match e {
                    SubmitError::Malformed { .. } | SubmitError::NotModalSubmit { .. } => {
                        Rejection::Malformed
                    }
                    _ => Rejection::SubmitInvalid,
                }),
        ),
        _ => None,
    }
}

/// Where a component run sends its responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wire {
    /// The interaction (every real run).
    Interaction,
    /// Nowhere (the `__testing::dispatch_detached` seam).
    Detached,
}

/// Runs one `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction through the
/// component pipeline (see the module docs) in span `noctis.interaction`.
pub(crate) async fn run_component(
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    inbound: Inbound,
    cancel: CancellationToken,
) {
    run_component_over(registry, app, inbound, cancel, Wire::Interaction).await;
}

/// [`run_component`] sending over `wire`.
#[expect(
    clippy::too_many_lines,
    reason = "one linear pass over the stages, each rejection answered where it happens"
)]
pub(crate) async fn run_component_over(
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    inbound: Inbound,
    cancel: CancellationToken,
    wire: Wire,
) {
    let shard = inbound.shard.number();
    let received_at = inbound.received_at;
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
        application_id,
        id: interaction_id,
        token,
        kind,
        data,
        locale,
        guild_locale,
        guild_id,
        channel,
        authorizing_integration_owners: owners,
        ..
    } = *interaction;
    let channel_id = channel.as_ref().map(|c| c.id);
    let answer = Rejecter {
        span: &span,
        shard,
        app: &app,
        locale: notice::locale(locale.as_deref(), guild_locale.as_deref()),
    };
    let transport = match wire {
        Wire::Interaction => Transport::Interaction(InteractionTarget {
            app: Arc::clone(&app),
            application_id,
            interaction_id,
            token,
            received_at,
            channel_id,
        }),
        Wire::Detached => Transport::Detached,
    };
    // A user-install-only interaction may send 5 follow-ups (INT-11).
    let cap = (owners.guild.is_none() && owners.user.is_some()).then_some(USER_INSTALL_FOLLOWUPS);
    let shared_for = |transport, declared| {
        Shared::declared(
            transport,
            declared,
            SourceMessage::from_interaction(raw.as_bytes()),
            app.limits.clone(),
            cap,
        )
    };
    let notice_state = |transport| shared_for(transport, Declared::Reply(Ephemerality::Ephemeral));
    // Stage 1: read the input.
    let input = match read_input(kind, data, raw.as_bytes()) {
        None => return,
        Some(Ok(input)) => input,
        Some(Err(rejection)) => {
            return answer
                .reject(None, rejection, &notice_state(transport))
                .await;
        }
    };
    // Stage 2: look up the Route.
    let Ok(decoded) = noctis_codec::decode(input.custom_id()) else {
        return answer
            .reject(None, Rejection::Malformed, &notice_state(transport))
            .await;
    };
    let route = decoded.route();
    let family = match &input {
        Input::Component { kind, .. } => Family::Component(*kind),
        Input::Modal(_) => Family::Modal,
    };
    let entry = match resolve(&registry, &decoded, family) {
        Ok(entry) => entry,
        Err(rejection) => {
            return answer
                .reject(Some(route), rejection, &notice_state(transport))
                .await;
        }
    };
    let target = match (&entry.handler, &input) {
        (RouteHandler::Component(handler), Input::Component { .. }) => Target::Component(handler),
        (RouteHandler::Modal(handler), Input::Modal(submission)) => {
            Target::Modal(handler, submission)
        }
        _ => {
            return answer
                .reject(
                    Some(route),
                    Rejection::UnknownRoute,
                    &notice_state(transport),
                )
                .await;
        }
    };
    span.record("module", entry.module);
    span.record("route", route);
    let Some(user_id) = author else {
        return answer
            .reject(Some(route), Rejection::NoAuthor, &notice_state(transport))
            .await;
    };
    // Stage 3: build the context and the responder state.
    let parts = CtxInner {
        module: entry.module,
        shard,
        guild_id,
        channel_id,
        user_id,
        user_locale: locale,
        guild_locale,
        app: Some(Arc::clone(&app)),
        ..CtxInner::detached_parts(Ephemerality::Public)
    };
    let state = shared_for(transport, target.response());
    let report = Report {
        app: &app,
        origin: ErrorOrigin::Component,
        shard: Some(shard),
        guild_id,
        module: entry.module,
        handler: entry.handler.name(),
        event_type: Some(INTERACTION_CREATE),
        locale: answer.locale,
    };
    if cancel.is_cancelled() {
        return run_started(&span, Some(&cancel), state, None, None, report).await;
    }
    // Stage 4: decode the struct and start the handler.
    let at = Instant::now();
    let handle = Arc::clone(&state);
    let started = span.in_scope(|| {
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let keys = app.payload_keys.as_deref().map(|k| k as &dyn Verify);
            target.start(parts, &decoded, handle, keys)
        }))
    });
    let future = match started {
        Ok(Err(rejection)) => return answer.reject(Some(route), rejection, &state).await,
        Ok(Ok(future)) => Ok(future),
        Err(panic) => Err(panic),
    };
    // Stage 5: arm the auto-defer.
    let armed = Armed {
        handler: report.handler,
        module: report.module,
        auto_defer: target.auto_defer(),
    };
    let timer = arm(&app, &state, &armed, received_at, &span);
    // Stage 6: run and answer.
    let started = Started { at, future };
    run_started(
        &span,
        Some(&cancel),
        state,
        Some(started),
        Some(timer),
        report,
    )
    .await;
}
