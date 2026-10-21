//! Middleware Matrix 12, 13, 36-38, 40, 41 and 43: the Middleware values,
//! the run's Extensions and their extraction.

use std::sync::Arc;

use tracing::Span;

use super::*;
use crate::command::{CommandCtx, CtxInner};
use crate::kind::{__ext, __ext_opt};
use crate::responder::Ephemerality;
use crate::test_support::Capture;

#[derive(Debug, PartialEq, Eq)]
struct A(u32);

#[derive(Debug, PartialEq, Eq)]
struct B;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Marker;

#[derive(Debug)]
struct Secret {
    #[expect(dead_code, reason = "only its Debug must never be printed")]
    secret: &'static str,
}

async fn cont(_: BeforeCtx) -> HandlerResult<Flow> {
    Ok(Flow::Continue)
}

async fn noop(_: AfterCtx) {}

fn info() -> RunInfo {
    RunInfo {
        invocation: Invocation::Command { path: "ping" },
        module: "m",
        handler: "bot::Ping",
        shard: 0,
        guild_id: None,
        channel_id: None,
        user_id: None,
        app: None,
    }
}

fn chain(before: &[BeforeMiddleware]) -> Chain {
    Chain::default().extend(before, &[])
}

/// A command context reading `around`'s run.
fn ctx(around: &Around) -> CommandCtx {
    CommandCtx::from_parts(CtxInner {
        run: around.run(),
        ..CtxInner::detached_parts(Ephemerality::Public)
    })
}

/// Matrix 12 and 13: `provides` keeps a type once; the last scope call
/// wins; `Debug` names the Middleware, its scope and its types.
#[test]
fn provides_dedupe_and_scope() {
    let mw = Middleware::before(cont)
        .provides::<A>()
        .provides::<A>()
        .provides::<B>();
    assert_eq!(mw.provides.len(), 2);
    assert_eq!(mw.scope, Scope::All);
    let debug = format!("{mw:?}");
    assert!(
        debug.contains("cont") && debug.contains("scope: \"all\""),
        "{debug}"
    );
    assert!(
        debug.contains("middleware::tests::A") && debug.contains("middleware::tests::B"),
        "{debug}"
    );
    assert!(mw.name().ends_with("tests::cont"), "{}", mw.name());
    let mw = mw.events_only().skip_events();
    assert_eq!(mw.scope, Scope::SkipEvents);
    assert!(mw.provides_for(TypeId::of::<A>(), false));
    assert!(!mw.provides_for(TypeId::of::<A>(), true));
    assert!(!mw.provides_for(TypeId::of::<Marker>(), false));
    let mw = mw.skip_events().events_only();
    assert_eq!(mw.scope, Scope::EventsOnly);

    let after = Middleware::after(noop).skip_events().events_only();
    assert_eq!(after.scope, Scope::EventsOnly);
    let debug = format!("{after:?}");
    assert!(
        debug.contains("noop") && debug.contains("events_only"),
        "{debug}"
    );
    assert!(Scope::All.covers(true) && Scope::All.covers(false));
    assert!(Scope::EventsOnly.covers(true) && !Scope::EventsOnly.covers(false));
    assert!(!Scope::SkipEvents.covers(true) && Scope::SkipEvents.covers(false));
}

/// Matrix 36 and 38: an inserted value reaches the extraction; an absent
/// optional one is `None`, an absent required one `ExtensionMissing`.
#[tokio::test]
async fn ext_value_and_optional_absent() {
    let insert = Middleware::before(|ctx: BeforeCtx| async move {
        assert!(!ctx.contains::<A>());
        ctx.insert(A(7));
        assert_eq!(ctx.get::<A>().as_deref(), Some(&A(7)));
        Ok(Flow::Continue)
    })
    .provides::<A>();
    let around = Around::new(&chain(&[insert]), info, false);
    assert!(around.has_before());
    let ctx = ctx(&around);
    assert!(__ext_opt::<A, _>(&ctx).is_none(), "not frozen yet");
    assert!(matches!(
        around.before(&Span::none(), None).await,
        Before::Continue
    ));
    let a = __ext::<A, _>(&ctx, "bot::Ping").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(*a, A(7));
    assert_eq!(a.as_ref(), &A(7));
    assert_eq!(format!("{:?}", a.clone()), "Ext(A(7))");
    assert_eq!(*a.into_arc(), A(7));
    assert!(__ext_opt::<B, _>(&ctx).is_none());
    let missing = __ext::<B, _>(&ctx, "bot::Ping").unwrap_err();
    let missing = missing
        .as_error()
        .downcast_ref::<ExtensionMissing>()
        .unwrap_or_else(|| panic!("not an ExtensionMissing"));
    assert_eq!(missing.handler(), "bot::Ping");
    assert!(missing.extension().ends_with("tests::B"));
    assert_eq!(
        missing.to_string(),
        format!(
            "handler `bot::Ping` takes `Ext<{}>`, but no Middleware inserted it in this run",
            std::any::type_name::<B>()
        )
    );
    let extensions = around.extensions();
    assert_eq!((extensions.len(), extensions.is_empty()), (1, false));
    assert!(extensions.contains::<A>() && !extensions.contains::<B>());
    assert_eq!(extensions.get::<A>(), Some(&A(7)));
}

/// Matrix 37: the child's insert replaces the parent's, with one `debug`
/// naming both Middleware and never the value.
#[tokio::test]
async fn replace_is_one_debug() {
    async fn parent(ctx: BeforeCtx) -> HandlerResult<Flow> {
        ctx.insert(A(1));
        Ok(Flow::Continue)
    }
    async fn child(ctx: BeforeCtx) -> HandlerResult<Flow> {
        ctx.insert(A(2));
        Ok(Flow::Continue)
    }
    let root = Chain::default().extend(&[Middleware::before(parent)], &[]);
    let chain = root.extend(&[Middleware::before(child)], &[]);
    let around = Around::new(&chain, info, false);
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    around.before(&Span::none(), None).await;
    assert_eq!(around.extensions().get::<A>(), Some(&A(2)));
    let lines = capture.lines_with(&["noctis::middleware", "extension replaced"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    let line = &lines[0];
    assert!(
        line.contains("::parent\"") && line.contains("::child\""),
        "{line}"
    );
    assert!(
        line.contains("tests::A") && !line.contains("A(1)"),
        "{line}"
    );
}

/// Matrix 40: an insert from a clone kept past the before chain is ignored
/// with a `debug`.
#[tokio::test]
async fn late_insert_is_ignored() {
    let kept: Arc<Mutex<Option<BeforeCtx>>> = Arc::default();
    let keep = Arc::clone(&kept);
    let mw = Middleware::before(move |ctx: BeforeCtx| {
        let keep = Arc::clone(&keep);
        async move {
            *keep.lock().unwrap() = Some(ctx);
            Ok(Flow::Continue)
        }
    });
    let around = Around::new(&chain(&[mw]), info, false);
    around.before(&Span::none(), None).await;
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let late = kept.lock().unwrap().take().unwrap();
    tokio::spawn(async move { late.insert(A(9)) })
        .await
        .unwrap();
    assert!(around.extensions().is_empty());
    assert_eq!(
        capture
            .lines_with(&["noctis::middleware", "after the before-Middleware ended"])
            .len(),
        1,
        "{}",
        capture.text()
    );
}

/// Matrix 41: a zero-sized Extension works.
#[tokio::test]
async fn zero_sized_extension() {
    let mw = Middleware::before(|ctx: BeforeCtx| async move {
        ctx.insert(Marker);
        Ok(Flow::Continue)
    });
    let around = Around::new(&chain(&[mw]), info, false);
    around.before(&Span::none(), None).await;
    let marker = __ext::<Marker, _>(&ctx(&around), "h").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(*marker, Marker);
}

/// Matrix 43: `Debug` of the map, the contexts and the run names the types,
/// never a value.
#[tokio::test]
async fn debug_never_prints_values() {
    let mw = Middleware::before(|ctx: BeforeCtx| async move {
        ctx.insert(Secret { secret: "hunter2" });
        let debug = format!("{ctx:?}");
        assert!(
            debug.contains("bot::Ping") && !debug.contains("hunter2"),
            "{debug}"
        );
        Ok(Flow::Continue)
    });
    let after = Middleware::after(|ctx: AfterCtx| async move {
        let debug = format!("{ctx:?}");
        assert!(
            debug.contains("Secret") && !debug.contains("hunter2"),
            "{debug}"
        );
        assert_eq!(ctx.outcome(), RunOutcome::Completed);
        assert!(ctx.get::<Secret>().is_some());
    });
    let chain = Chain::default().extend(&[mw], &[after]);
    let around = Around::new(&chain, info, false);
    around.before(&Span::none(), None).await;
    let debug = format!("{:?}", around.extensions());
    assert!(debug.contains("tests::Secret"), "{debug}");
    assert!(!debug.contains("hunter2"), "{debug}");
    around
        .after(&Span::none(), None, RunOutcome::Completed, |name, panic| {
            panic!("{name} panicked: {panic}")
        })
        .await;
    assert_eq!(format!("{:?}", Extensions::empty()), "{}");
}

/// A chain covering nothing allocates no run; the outcomes map as
/// documented.
#[test]
fn empty_chain_and_outcomes() {
    let events_only = Chain::default().extend(&[Middleware::before(cont).events_only()], &[]);
    let around = Around::new(&events_only, info, false);
    assert!(around.run().is_none() && !around.has_before());
    assert!(around.extensions().is_empty());
    assert!(Around::new(&events_only, info, true).run().is_some());
    assert!(!Chain::default().covers(false));
    assert_eq!(RunOutcome::of(&Ok(())), RunOutcome::Completed);
    assert_eq!(
        RunOutcome::of(&Err(HandlerError::user(MessageKey::new("k")))),
        RunOutcome::UserError
    );
    assert_eq!(
        RunOutcome::of(&Err(HandlerError::new(fmt::Error))),
        RunOutcome::Failed
    );
    for outcome in [
        RunOutcome::Stopped,
        RunOutcome::UserError,
        RunOutcome::Failed,
        RunOutcome::Panicked,
    ] {
        assert!(!outcome.is_completed());
    }
    assert!(RunOutcome::Completed.is_completed());
}
