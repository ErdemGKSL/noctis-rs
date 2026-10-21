//! The component pipeline (Stories 7.1, 7.2 and 7.3; AD-5, AD-19, AD-23,
//! AD-25, AD-29; CID-04, CID-06, INT-01, INT-04; FR-18, FR-60): a
//! `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction runs the handler of its
//! Route on the decoded struct, with a responder over the interaction.
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
//!    declared `response` (a component's attribute, or a submit's
//!    `#[modal(.., response = ..)]`, default `update`). A submit's state
//!    refuses `show_modal` with `ModalUnavailable` and no request (INT-04).
//! 4. **Decode the struct and start the handler:** the Payload is decoded
//!    once, verifying a `signed` Route's tag with the App's payload key ring
//!    (current or previous key); a modal's inputs are decoded from that
//!    verified Payload and the submission (`ModalFromPayload`). A Ref id
//!    (Story 7.5, FR-17) is resolved in the App's namespaced `RefStore`
//!    (`take` for a `one_shot` Route, else `get`) and the struct decoded
//!    from the stored record: a missing record goes to the Expired Handler
//!    chain, a record of another route (or one that does not parse) is
//!    `forged`, one of another schema is stale, and a store error is
//!    `ref_store`; each completed lookup counts one
//!    `noctis_ref_lookups_total{app, result}` (`hit`, `miss`, `stale`,
//!    `forged`). The Ref is never logged. A stale id
//!    (another `schema`, body kind or `N`/`T` flag set of the Route) or a
//!    Ref id whose record is missing runs the Expired Handler chain
//!    instead: a stale id the Route's `#[on_stale]`,
//!    then its `#[on_expired]` with [`Expired::Stale`]; a Ref id only its
//!    `#[on_expired]` with [`Expired::RefMissing`]; then the App's
//!    `expired_handler`; else the default `noctis.expired` ephemeral notice
//!    (one type-4 callback, no timer). Each is one `debug` on target
//!    `noctis::dispatch` naming the `reason` and the `hook`.
//! 5. **Arm the auto-defer** (shared with Commands): at the ack deadline a
//!    type-6 callback under `response = update` when a source message
//!    exists, else type 5 (`EPHEMERAL` only for `response = ephemeral`); an
//!    `auto_defer = false` handler or hook (a component's or a
//!    `#[modal(.., auto_defer = false)]`) gets only the 2.9 s error log.
//!    The App's `expired_handler` is always auto-deferred.
//! 6. **Run and answer:** run the Module path's before-Middleware (not for
//!    the Expired Handler chain), then the Route Guards (AD-23 stage 8;
//!    `owner`, `permissions`, then a custom `guard = path`, the first
//!    denial winning; never for the Expired Handler chain or the detached
//!    seams): a denial is one `debug` on `noctis::guard`, puts a taken
//!    `one_shot` record back under its Ref (`put_if_absent` with the Route's
//!    `ttl`, else the App's `ref_ttl`), sends the localized `noctis.guard.*`
//!    (or custom) ephemeral notice and runs the after-Middleware with
//!    `RunOutcome::Denied`, with no duration, counter or report. A Guard
//!    that errs, panics or is cancelled is reported like a handler, but the
//!    taken record goes back too and no duration is recorded: the body never
//!    started. Then the handler (or hook) runs under `catch_unwind`, raced
//!    against the shutdown cancel token, and send what it returned per the
//!    declared `response`; an error or a panic sends the localized
//!    ephemeral notice, then a user condition is logged at `debug`; any
//!    other error, a panic, a failed send or a cancellation is counted and
//!    reported to the Component Error Sink, after the after-Middleware ran
//!    (as in the Command pipeline's stages 7 and 10).
//!
//! Every other input that does not reach a handler is a [`Rejection`]: one
//! `warn` on target `noctis::dispatch` and the CID-06 generic ephemeral
//! notice; no handler run and no panic. The checks before the Payload
//! decode (malformed, unknown Route, kind mismatch, no author) win over a
//! stale id. Nothing is stored per message or component: the one
//! auto-defer task per interaction ends when the handler run ends.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use noctis_codec::{DecodeError, Decoded, RawBody, Ref, Verify};
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

use super::{
    Armed, INTERACTION_CREATE, Report, Restore, Started, USER_INSTALL_FOLLOWUPS, arm, run_started,
};
use crate::command::CtxInner;
use crate::component::{
    Begin, ComponentCtx, ComponentHandler, ComponentKind, Expired, ExpiredCtx, ExpiredLink,
    HandlerFuture, ModalHandler, ModalSubmitCtx,
};
use crate::handler::AppShared;
use crate::middleware::{Around, Invocation, RunInfo};
use crate::notice;
use crate::registry::{Registry, RouteEntry, RouteHandler};
use crate::responder::{
    Declared, Ephemerality, InteractionTarget, Shared, SourceMessage, Transport,
};
use crate::sink::ErrorOrigin;

/// Why a component or modal interaction did not reach a handler; its
/// [`as_str`](Self::as_str) is the `reason` of the one `warn` (or, for a
/// stale or Ref id, of the one `debug` of the Expired Handler chain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rejection {
    /// The `custom_id` failed codec phase 1, the click has no data or the
    /// modal submission is unparsable.
    Malformed,
    /// No handler has the Route id, or only one of the other family.
    UnknownRoute,
    /// The clicked component's type is not the handler's kind.
    KindMismatch,
    /// The id is from another shape or schema of the Route: the Expired
    /// Handler chain answers it.
    Stale,
    /// A bad or missing integrity tag, or one no key of the ring verifies.
    Forged,
    /// A Ref body whose record is missing or expired in the App's
    /// `RefStore` (or was already taken by a `one_shot` use): the Expired
    /// Handler chain answers it.
    RefUnresolved,
    /// The App's `RefStore` failed to look the Ref up.
    RefStore,
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
            Self::RefStore => "ref_store",
            Self::Undecodable => "undecodable",
            Self::SubmitInvalid => "submit_invalid",
            Self::NoAuthor => "no_author",
        }
    }

    /// Why a stale or Ref id goes to the Expired Handler chain; `None` for
    /// every other rejection (the CID-06 notice answers those).
    pub(crate) const fn expired(self) -> Option<Expired> {
        match self {
            Self::Stale => Some(Expired::Stale),
            Self::RefUnresolved => Some(Expired::RefMissing),
            _ => None,
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
    fn start(
        &self,
        parts: Arc<CtxInner>,
        decoded: &Decoded<'_>,
        shared: Arc<Shared>,
        keys: Option<&dyn Verify>,
    ) -> Result<Begin, Rejection> {
        match self {
            Self::Component(handler) => {
                handler.start(ComponentCtx::from_inner(parts), decoded, shared, keys)
            }
            Self::Modal(handler, submission) => handler.start(
                ModalSubmitCtx::from_inner(parts),
                decoded,
                submission,
                shared,
                keys,
            ),
        }
    }

    /// Starts the handler on the stored `record` of the id's Ref.
    fn start_record(
        &self,
        parts: Arc<CtxInner>,
        record: &[u8],
        shared: Arc<Shared>,
    ) -> Result<HandlerFuture, Rejection> {
        match self {
            Self::Component(handler) => {
                handler.start_record(ComponentCtx::from_inner(parts), record, shared)
            }
            Self::Modal(handler, submission) => handler.start_record(
                ModalSubmitCtx::from_inner(parts),
                record,
                submission,
                shared,
            ),
        }
    }

    /// Whether the Route's Refs are resolved with `take` (`one_shot`).
    fn one_shot(&self) -> bool {
        match self {
            Self::Component(handler) => handler.one_shot(),
            Self::Modal(handler, _) => handler.one_shot(),
        }
    }

    /// The Route's declared Ref `ttl`, if any.
    fn ref_ttl(&self) -> Option<std::time::Duration> {
        match self {
            Self::Component(handler) => handler.ref_ttl(),
            Self::Modal(handler, _) => handler.ref_ttl(),
        }
    }

    /// The link of the Expired Handler chain that answers `expired`.
    fn expired_link(&self, expired: Expired, app: &AppShared) -> ExpiredLink {
        let app = app.expired_handler.is_some();
        match self {
            Self::Component(handler) => handler.expired_link(expired, app),
            Self::Modal(handler, _) => handler.expired_link(expired, app),
        }
    }

    /// Starts the hook or App handler of `link` with a context of `parts`
    /// (and, for a hook, a responder over `shared`); `None` for the default
    /// link.
    fn start_expired(
        &self,
        link: ExpiredLink,
        parts: Arc<CtxInner>,
        expired: Expired,
        shared: Arc<Shared>,
        app: &AppShared,
    ) -> Option<HandlerFuture> {
        match (link, self) {
            (ExpiredLink::App, _) => {
                let handler = app.expired_handler.as_ref()?;
                let modal = matches!(self, Self::Modal(..));
                Some(handler(ExpiredCtx::from_inner(parts), expired, modal))
            }
            (_, Self::Component(handler)) => {
                handler.start_hook(link, ComponentCtx::from_inner(parts), expired, shared)
            }
            (_, Self::Modal(handler, _)) => {
                handler.start_hook(link, ModalSubmitCtx::from_inner(parts), expired, shared)
            }
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

/// Counts one completed Ref lookup of `app` with `result` (`hit`, `miss`,
/// `stale` or `forged`). The Ref itself is never a label.
fn count_lookup(app: &AppShared, result: &'static str) {
    metrics::counter!(
        "noctis_ref_lookups_total",
        "app" => Arc::clone(&app.app),
        "result" => result
    )
    .increment(1);
}

/// The `result` label of a resolved record's start: `forged` and `stale`
/// for a record not bound to this Route and schema, else `hit`.
const fn lookup_label(started: Result<(), Rejection>) -> &'static str {
    match started {
        Err(Rejection::Forged) => "forged",
        Err(Rejection::Stale) => "stale",
        _ => "hit",
    }
}

/// Looks the Ref `r` up in the App's namespaced `RefStore`: `take` for a
/// `one_shot` Route, else `get`. A missing record is counted (`miss`) and
/// is [`Rejection::RefUnresolved`]; a store error is
/// [`Rejection::RefStore`] and is not counted.
async fn lookup(app: &AppShared, r: Ref, one_shot: bool) -> Result<Bytes, Rejection> {
    let found = if one_shot {
        app.refs.take(r.as_str()).await
    } else {
        app.refs.get(r.as_str()).await
    };
    match found {
        Err(_) => Err(Rejection::RefStore),
        Ok(None) => {
            count_lookup(app, "miss");
            Err(Rejection::RefUnresolved)
        }
        Ok(Some(record)) => Ok(record),
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
    /// Logs `rejection`, then sends the CID-06 generic ephemeral notice over
    /// `shared`; a failed send is one `warn`.
    async fn reject(&self, route: Option<&str>, rejection: Rejection, shared: &Arc<Shared>) {
        log_rejection(self.span, self.shard, route, rejection);
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

impl Rejecter<'_> {
    /// Logs that the stale or Ref id `rejection` of `route` goes to `link`
    /// of the Expired Handler chain (one `debug`).
    fn log_expired(&self, route: &str, rejection: Rejection, link: ExpiredLink) {
        self.span.in_scope(|| {
            tracing::debug!(
                target: "noctis::dispatch",
                reason = rejection.as_str(),
                shard = self.shard,
                route,
                hook = link.as_str(),
                "component interaction expired"
            );
        });
    }

    /// The default Expired Handler: one type-4 ephemeral `noctis.expired`
    /// notice over `shared`, sent at once; a failed send is one `warn`.
    async fn expired_notice(&self, rejection: Rejection, shared: &Arc<Shared>) {
        let text = self
            .span
            .in_scope(|| notice::text(&*self.app.strings, self.locale, keys::EXPIRED));
        if let Err(error) = shared.notice(text).instrument(self.span.clone()).await {
            self.span.in_scope(|| {
                tracing::warn!(
                    target: "noctis::dispatch",
                    reason = rejection.as_str(),
                    %error,
                    "could not send the expired notice"
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
        member,
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
    // A modal submit (`Input::Modal`) never shows a modal: its state refuses
    // `show_modal` for the handler, the hooks and the notices (INT-04).
    let submit = kind == InteractionType::ModalSubmit;
    let shared_for = |transport, declared| {
        let build = if submit {
            Shared::declared_submit
        } else {
            Shared::declared
        };
        build(
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
    let invocation = match &target {
        Target::Component(_) => Invocation::Component {
            route: entry.handler.route(),
        },
        Target::Modal(..) => Invocation::Modal {
            route: entry.handler.route(),
        },
    };
    let around = Around::new(
        &entry.chain,
        || RunInfo {
            invocation,
            module: entry.module,
            handler: entry.handler.name(),
            shard,
            guild_id,
            channel_id,
            user_id: Some(user_id),
            app: Some(Arc::clone(&app)),
        },
        false,
    );
    let mut parts = CtxInner {
        // The Route id: `ExpiredCtx::route` (the component contexts do not
        // read it).
        path: entry.handler.route(),
        module: entry.module,
        shard,
        guild_id,
        channel_id,
        user_id,
        user_locale: locale,
        guild_locale,
        app: Some(Arc::clone(&app)),
        current_ref: match decoded.body() {
            RawBody::Ref(r) => Some(r),
            RawBody::Inline(_) => None,
        },
        run: around.run(),
        member_permissions: member.and_then(|m| m.permissions),
        ..CtxInner::detached_parts(Ephemerality::Public)
    };
    let state = shared_for(transport, target.response());
    // The response locale follows who sees the response (FR-43).
    parts.ephemerality = state.response_ephemerality();
    parts.shared = Some(Arc::clone(&state));
    let parts = Arc::new(parts);
    let mut report = Report {
        app: &app,
        origin: ErrorOrigin::Component,
        shard: Some(shard),
        guild_id,
        module: entry.module,
        handler: entry.handler.name(),
        event_type: Some(INTERACTION_CREATE),
        locale: answer.locale,
        around: Some(around),
        restore: None,
    };
    if cancel.is_cancelled() {
        return run_started(&span, Some(&cancel), state, None, None, report).await;
    }
    // Stage 4: decode the struct (resolving a Ref id's record) and start
    // the handler, or route a stale or missing-Ref id through the Expired
    // Handler chain.
    let at = Instant::now();
    let handle = Arc::clone(&state);
    let begun = span.in_scope(|| {
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let keys = app.payload_keys.as_deref().map(|k| k as &dyn Verify);
            target.start(Arc::clone(&parts), &decoded, handle, keys)
        }))
    });
    let started = match begun {
        Ok(Ok(Begin::Ready(future))) => Ok(Ok(future)),
        Ok(Ok(Begin::Ref(r))) => match lookup(&app, r, target.one_shot())
            .instrument(span.clone())
            .await
        {
            Err(rejection) => Ok(Err(rejection)),
            Ok(record) => {
                let handle = Arc::clone(&state);
                let started = span.in_scope(|| {
                    std::panic::catch_unwind(AssertUnwindSafe(|| {
                        target.start_record(Arc::clone(&parts), &record, handle)
                    }))
                });
                // A taken `one_shot` record goes back if the Guard phase
                // ends without the handler body starting.
                if target.one_shot() {
                    report.restore = Some(Restore {
                        r,
                        ttl: target.ref_ttl().unwrap_or(app.ref_ttl),
                        record,
                    });
                }
                let label = match &started {
                    Ok(result) => lookup_label(result.as_ref().map(drop).map_err(|r| *r)),
                    Err(_) => "hit",
                };
                count_lookup(&app, label);
                started
            }
        },
        Ok(Err(rejection)) => Ok(Err(rejection)),
        Err(panic) => Err(panic),
    };
    let (future, auto_defer) = match started {
        Ok(Ok(future)) => (Ok(future), target.auto_defer()),
        Err(panic) => (Err(panic), target.auto_defer()),
        Ok(Err(rejection)) => {
            let Some(expired) = rejection.expired() else {
                return answer.reject(Some(route), rejection, &state).await;
            };
            let link = target.expired_link(expired, &app);
            answer.log_expired(route, rejection, link);
            // The Expired Handler chain runs no Middleware.
            report.around = None;
            let handle = Arc::clone(&state);
            let started = span.in_scope(|| {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    target.start_expired(link, parts, expired, handle, &app)
                }))
            });
            let future = match started {
                Ok(Some(future)) => Ok(future),
                Ok(None) => return answer.expired_notice(rejection, &state).await,
                Err(panic) => Err(panic),
            };
            // The App handler takes no responder: always auto-deferred.
            (future, link == ExpiredLink::App || target.auto_defer())
        }
    };
    // Stage 5: arm the auto-defer.
    let armed = Armed {
        handler: report.handler,
        module: report.module,
        auto_defer,
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
