//! Story 8.4: `assert_memory_flat` (NFR-2, rows Mem1-Mem4 and Mem6).
//!
//! dhat allows one profiler per process and its counters are process-wide,
//! so this binary installs the dhat allocator and holds exactly one
//! `#[test]`, which builds its own multi-thread runtime.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis_testkit::{TestApp, TestError, assert_memory_flat};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn view(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

#[derive(SlashCommand)]
#[command("ping", description = "Ping")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(view("pong")).await
    }
}

/// Keeps 1 KiB per run: the leak `assert_memory_flat` must catch.
#[derive(SlashCommand)]
#[command("hoard", description = "Keeps state per run")]
struct Hoard;

impl Hoard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.state::<Mutex<Vec<Vec<u8>>>>()
            .unwrap()
            .lock()
            .unwrap()
            .push(vec![7; 1024]);
        ctx.respond(view("kept")).await
    }
}

async fn ping(app: &TestApp) -> Result<(), TestError> {
    app.command("ping").run().await.map(drop)
}

async fn scenarios() -> Result<(), TestError> {
    let mut builder = App::builder();
    builder
        .state(Mutex::new(Vec::<Vec<u8>>::new()))
        .module(Module::new("m").add::<Ping>().add::<Hoard>());
    let app = TestApp::build(&mut builder).await?;

    // Mem1: a stateless handler stays flat.
    let report = assert_memory_flat(&app, 200, ping).await?;
    assert_eq!((report.runs, report.warm_up), (200, 20));

    // Mem2: 1 KiB kept per run is caught.
    let err = assert_memory_flat(&app, 200, |app| async move {
        app.command("hoard").run().await.map(drop)
    })
    .await
    .unwrap_err();
    let TestError::MemoryGrew {
        before,
        after,
        runs: 200,
    } = err
    else {
        panic!("{err}");
    };
    assert!(after - before >= 200 * 1024, "{before} -> {after}");

    // Mem3: at least 10 runs.
    let err = assert_memory_flat(&app, 9, ping).await.unwrap_err();
    assert!(matches!(err, TestError::TooFewRuns { n: 9 }), "{err}");
    let report = assert_memory_flat(&app, 10, ping).await?;
    assert_eq!((report.runs, report.warm_up), (10, 10));

    // Mem4: one profiler at a time.
    let (first, second) = tokio::join!(
        assert_memory_flat(&app, 10, ping),
        assert_memory_flat(&app, 10, ping)
    );
    let busy = [&first, &second]
        .iter()
        .filter(|r| matches!(r, Err(TestError::ProfilerBusy)))
        .count();
    assert_eq!(busy, 1, "{first:?} {second:?}");
    assert!(first.is_ok() || second.is_ok(), "{first:?} {second:?}");

    // Mem6: the first error of `f` is returned as is.
    let calls = AtomicUsize::new(0);
    let err = assert_memory_flat(&app, 10, |app| {
        let call = calls.fetch_add(1, Ordering::Relaxed) + 1;
        async move {
            if call == 3 {
                Err(TestError::Unsupported("run 3 fails"))
            } else {
                ping(app).await
            }
        }
    })
    .await
    .unwrap_err();
    assert!(
        matches!(err, TestError::Unsupported("run 3 fails")),
        "{err}"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);

    app.stop().await
}

#[test]
fn assert_memory_flat_scenarios() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scenarios())
        .unwrap();
}
