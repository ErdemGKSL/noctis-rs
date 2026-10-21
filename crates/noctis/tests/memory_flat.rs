//! Memory stays flat with N clicks (Story 7.1 Matrix 30, Story 7.2 Matrix
//! 57; NFR-2): dispatch keeps no closure, task, timer or map entry per
//! message or component; the one auto-defer task of a click ends with it.
//!
//! The handler `update`s the clicked message through its responder over a
//! detached transport (no request). A dhat counting allocator reads the
//! live heap (`curr_bytes`) after 100 warm-up and 1 000 clicks of one
//! Route, then after 10 000 more clicks with distinct interaction ids and
//! Payloads; the second reading may exceed the first by at most 1 KiB. The
//! loop yields after each click, as a running App does, so the runtime
//! reaps the aborted auto-defer task.
//!
//! dhat allows one profiler per process, so this binary holds exactly one
//! `#[test]`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use noctis::__private::core::__testing::dispatch_detached;
use noctis::Runtime;
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

static CLICKS: AtomicUsize = AtomicUsize::new(0);

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        assert_eq!(ctx.module(), "shop");
        CLICKS.fetch_add(1, Ordering::Relaxed);
        let panel = Components::new().push(TextDisplay::new(format!("Bought {}.", self.item)));
        Ok(r.update(panel).await?.done())
    }
}

/// A click on `Buy { item: n }` with interaction id `1000 + n`.
fn click(n: u64) -> String {
    let custom_id = noctis::__private::codec::encode::<Buy>(
        &Buy { item: n },
        &noctis::__private::codec::Parts::new(),
    )
    .unwrap();
    format!(
        r#"{{"id":"{}","application_id":"7","type":3,"token":"tok","version":1,
        "authorizing_integration_owners":{{}},"entitlements":[],
        "user":{{"id":"3","username":"u","discriminator":"0000","avatar":null,"global_name":null}},
        "message":{{"id":"77","channel_id":"55","flags":32768,"content":"","tts":false,
        "author":{{"id":"7","username":"bot","discriminator":"0","avatar":null}},
        "timestamp":"2026-10-20T12:00:00.000000+00:00","edited_timestamp":null,
        "mention_everyone":false,"mentions":[],"mention_roles":[],"attachments":[],
        "embeds":[],"pinned":false,"type":0}},
        "data":{{"custom_id":"{custom_id}","component_type":2}}}}"#,
        1000 + n
    )
}

#[test]
fn memory_stays_flat_with_clicks() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-memory-flat.json"
        ))
        .build();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .module(Module::new("shop").add::<Buy>());
        let app = builder.build().unwrap();
        for n in 0..1_100 {
            dispatch_detached(&app, &click(n)).await;
            tokio::task::yield_now().await;
        }
        let first = dhat::HeapStats::get().curr_bytes;
        for n in 1_100..11_100 {
            dispatch_detached(&app, &click(n)).await;
            tokio::task::yield_now().await;
        }
        let second = dhat::HeapStats::get().curr_bytes;
        assert_eq!(CLICKS.load(Ordering::Relaxed), 11_100);
        assert!(
            second <= first + 1024,
            "live heap grew from {first} to {second} bytes over 10 000 clicks"
        );
    });
}
