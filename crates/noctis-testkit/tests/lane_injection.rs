//! The AD-10 injection path (Story 16.1, Matrix rows 2, 4 and 5): a
//! `TestApp` exposes the `LaneSender` its shard tasks push into, a
//! pre-parsed `Inbound` pushed on it reaches the App's handlers, and a push
//! after `stop()` is discarded without a panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use noctis::events::{EventCtx, MessageDelete};
use noctis::prelude::*;
use noctis_gateway::{Inbound, InboundKind, InboundSink as _};
use noctis_testkit::TestApp;
use twilight_model::gateway::ShardId;
use twilight_model::gateway::event::Event;
use twilight_model::id::Id;

/// The bound on the handler wait.
const WAIT: Duration = Duration::from_secs(5);

/// The last message id the handler saw and how many runs it had.
#[derive(Default)]
struct Seen {
    id: AtomicU64,
    runs: AtomicU64,
}

#[derive(EventHandler)]
struct OnDelete;

impl OnDelete {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        if let Some(seen) = ctx.state::<Arc<Seen>>() {
            seen.id.store(ev.id.get(), Ordering::Release);
            seen.runs.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
}

/// A pre-parsed `MESSAGE_DELETE` of message `id` on shard 0.
fn delete(id: u64) -> Inbound {
    Inbound {
        shard: ShardId::ONE,
        received_at: tokio::time::Instant::now(),
        kind: InboundKind::Event(Box::new(Event::MessageDelete(MessageDelete {
            id: Id::new(id),
            channel_id: Id::new(2),
            guild_id: Some(Id::new(1)),
        }))),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_injected_inbound_reaches_the_handler() {
    let seen = Arc::new(Seen::default());
    let mut builder = App::builder();
    builder
        .state(Arc::clone(&seen))
        .module(Module::new("inject").add::<OnDelete>());
    let app = TestApp::build(&mut builder).await.unwrap();

    let sender = app.lane_sender().expect("the run's lane sender");
    sender.push(delete(4242));
    tokio::time::timeout(WAIT, async {
        while seen.runs.load(Ordering::Acquire) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the handler ran");
    assert_eq!(seen.id.load(Ordering::Acquire), 4242);
    assert_eq!(seen.runs.load(Ordering::Acquire), 1);

    app.stop().await.unwrap();
    // The App's lanes are closed: the push is discarded, no handler runs.
    sender.push(delete(4243));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seen.runs.load(Ordering::Acquire), 1);
}
