//! Story 17.2: the track events are hook events a `#[derive(Hook)]` handler
//! takes, and an App with voice and such a Module builds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::lifecycle::{ExtensionEvent, HookCtx, HookEvent};
use noctis::prelude::*;
use noctis::voice::{EndReason, QueueEmpty, TrackEnd, TrackError, TrackStart, VoiceConfig};

/// Logs the start of every track.
#[derive(Hook)]
struct NowPlaying;

impl NowPlaying {
    #[handler]
    async fn handle(self, _ctx: HookCtx, event: TrackStart) -> HandlerResult<()> {
        tracing::info!(guild = %event.guild, track = %event.track.id(), "now playing");
        Ok(())
    }
}

/// Counts the ends.
#[derive(Hook)]
struct Ended;

impl Ended {
    #[handler]
    async fn handle(self, _ctx: HookCtx, event: TrackEnd) -> HandlerResult<()> {
        assert!(matches!(
            event.reason,
            EndReason::Finished | EndReason::Skipped | EndReason::Stopped
        ));
        Ok(())
    }
}

#[test]
fn names() {
    assert_eq!(<TrackStart as HookEvent>::NAME, "VOICE_TRACK_START");
    assert_eq!(<TrackEnd as ExtensionEvent>::NAME, "VOICE_TRACK_END");
    assert_eq!(<TrackError as HookEvent>::NAME, "VOICE_TRACK_ERROR");
    assert_eq!(<QueueEmpty as HookEvent>::NAME, "VOICE_QUEUE_EMPTY");
    assert_eq!(EndReason::Skipped.as_str(), "skipped");
}

#[tokio::test]
async fn derive_hook_on_track_events_builds() {
    let module = Module::new("music").add::<NowPlaying>().add::<Ended>();
    let debug = format!("{module:?}");
    assert!(
        debug.contains("NowPlaying") && debug.contains("Ended"),
        "{debug}"
    );
    let mut builder = App::builder();
    builder.module(module).voice(VoiceConfig::default());
    let app = builder.build().unwrap();
    assert!(app.modules().ids().any(|id| id == "music"));
}
