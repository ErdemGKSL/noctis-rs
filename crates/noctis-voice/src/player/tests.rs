//! The Songbird player's slot on a standalone (unconnected) `Call`.

use std::time::{Duration, Instant};

use super::*;
use crate::track::{Track, TrackId};

fn player() -> SongbirdPlayer {
    let id = |n| std::num::NonZeroU64::new(n).unwrap_or_else(|| panic!("non-zero"));
    let call = Call::standalone(id(9), id(1));
    SongbirdPlayer::new(
        Arc::new(tokio::sync::Mutex::new(call)),
        reqwest::Client::new(),
        Weak::new(),
    )
}

fn info() -> TrackInfo {
    Track::file("a.wav")
        .unwrap_or_else(|e| panic!("{e}"))
        .into_info(TrackId::new(1))
}

/// `play` hands the track to the driver; `stop` resets the token and drops
/// the handle.
#[tokio::test]
async fn play_then_stop_clears_the_slot() {
    let player = player();
    player.play(1, &info(), 1.0);
    let deadline = Instant::now() + Duration::from_secs(15);
    while player.slot().handle.is_none() {
        assert!(Instant::now() < deadline, "the handle never arrived");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    player.stop(2); // stale: ignored
    assert_eq!(player.slot().token, 1);
    player.stop(1);
    let slot = player.slot();
    assert_eq!(slot.token, 0);
    assert!(slot.handle.is_none());
}

/// A pause before the handle exists is recorded for the play task.
#[tokio::test]
async fn pause_before_the_handle_is_recorded() {
    let player = player();
    player.play(1, &info(), 1.0);
    player.pause(1);
    let slot = player.slot();
    assert!(slot.handle.is_none(), "the play task has not run yet");
    assert!(slot.paused);
}
