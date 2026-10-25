//! The Songbird player on a standalone (unconnected) `Call`: the slot, the
//! input preparation and its failures, and stale preparations.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use twilight_model::id::Id;

use super::*;
use crate::config::VoiceConfig;
use crate::media::media_client;
use crate::test_support::MediaServer;
use crate::track::{Track, TrackId};

/// The bound on every wait.
const WAIT: Duration = Duration::from_secs(15);

fn call() -> Arc<tokio::sync::Mutex<Call>> {
    let id = |n| std::num::NonZeroU64::new(n).unwrap_or_else(|| panic!("non-zero"));
    Arc::new(tokio::sync::Mutex::new(Call::standalone(id(9), id(1))))
}

fn media() -> reqwest::Client {
    media_client(&VoiceConfig::default()).unwrap_or_else(|e| panic!("{e}"))
}

fn player() -> SongbirdPlayer {
    SongbirdPlayer::new(call(), media(), Weak::new(), Arc::default())
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A playable track (the WAV-free Ogg Opus fixture).
fn good() -> Track {
    Track::file(fixture("sine-stereo-20ms.opus")).unwrap_or_else(|e| panic!("{e}"))
}

fn info(track: Track, id: u64) -> TrackInfo {
    track.into_info(TrackId::new(id))
}

async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A queue playing on a `SongbirdPlayer`, and that player's slot.
fn queue() -> (Arc<QueueCell>, Arc<Mutex<Slot>>) {
    let mut slot = None;
    let queue = Arc::new_cyclic(|queue: &Weak<QueueCell>| {
        let player = SongbirdPlayer::new(call(), media(), Weak::clone(queue), Arc::default());
        slot = Some(Arc::clone(&player.slot));
        QueueCell::new(Id::new(1), 10, Box::new(player), Arc::default(), None)
    });
    (queue, slot.unwrap_or_else(|| panic!("the slot")))
}

fn title(queue: &QueueCell) -> Option<String> {
    queue
        .read(crate::queue::Queue::now_playing)
        .flatten()
        .and_then(|t| t.title().map(str::to_owned))
}

/// `play` prepares the input and hands it to the driver; `stop` resets the
/// token and drops the handle.
#[cfg(feature = "ogg")]
#[tokio::test]
async fn play_then_stop_clears_the_slot() {
    let player = player();
    player.play(1, &info(good(), 1), 1.0);
    until("the handle", || player.slot().handle.is_some()).await;
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
    player.play(1, &info(good(), 1), 1.0);
    player.pause(1);
    let slot = player.slot();
    assert!(slot.handle.is_none(), "the play task has not run yet");
    assert!(slot.paused);
}

/// A successful preparation plays, then reports the track playable once
/// (`TrackStart`), since a parsed input fires no driver `Playable`.
#[cfg(feature = "ogg")]
#[tokio::test]
async fn prepared_track_starts() {
    let (queue, slot) = queue();
    queue
        .enqueue(info(good().title("a"), 1))
        .unwrap_or_else(|e| panic!("{e}"));
    until("TrackStart", || queue.started()).await;
    assert!(lock(&slot).handle.is_some());
}

/// Every probe failure is the track's error with the `MakePlayableError`
/// text; nothing reaches the driver and the next track starts.
#[cfg(feature = "ogg")]
#[tokio::test(flavor = "multi_thread")]
async fn probe_failures_are_track_errors() {
    let zeros = std::env::temp_dir().join(format!("noctis-voice-zeros-{}.bin", std::process::id()));
    std::fs::write(&zeros, [0_u8; 1024]).unwrap_or_else(|e| panic!("{e}"));
    let server = MediaServer::start().await;
    let bad = [
        Track::file(&zeros),
        Track::file("/nonexistent/noctis-voice/missing.opus"),
        Track::url(server.url("/missing")),
        Track::command("sh", ["-c", "exit 0"]),
    ];
    for (n, track) in bad.into_iter().enumerate() {
        let track = track.unwrap_or_else(|e| panic!("{e}"));
        let error = frames::prepare(
            track.clone().into_info(TrackId::new(9)).input(&media()),
            FrameCounters::noop(),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("case {n} is unplayable"));
        assert!(
            error.starts_with("failed to make track playable"),
            "case {n}: {error}"
        );

        let (queue, slot) = queue();
        queue
            .enqueue(info(track.title("bad"), 1))
            .unwrap_or_else(|e| panic!("{e}"));
        queue
            .enqueue(info(good().title("next"), 2))
            .unwrap_or_else(|e| panic!("{e}"));
        until(&format!("case {n}: the next track"), || {
            title(&queue).as_deref() == Some("next") && queue.started()
        })
        .await;
        let slot = lock(&slot);
        assert_eq!(slot.token, 2, "case {n}");
        assert!(slot.handle.is_some(), "case {n}");
    }
    let _ = std::fs::remove_file(zeros);
}

/// A stop while the input is prepared drops it: no `play_only`, no
/// `TrackStart`.
#[cfg(feature = "ogg")]
#[tokio::test]
async fn stale_preparation_after_stop() {
    let (queue, slot) = queue();
    queue
        .enqueue(info(good().title("a"), 1))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(queue.stop(), Ok(1));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(lock(&slot).handle.is_none(), "the stale input was dropped");
    assert!(!queue.started());
}

/// A skip while the input is prepared: the skipped track never reaches the
/// driver, the next one plays.
#[cfg(feature = "ogg")]
#[tokio::test]
async fn stale_preparation_after_skip() {
    let (queue, slot) = queue();
    for (n, name) in [(1, "a"), (2, "b")] {
        queue
            .enqueue(info(good().title(name), n))
            .unwrap_or_else(|e| panic!("{e}"));
    }
    assert!(queue.skip().unwrap_or_else(|e| panic!("{e}")).is_some());
    until("b starts", || queue.started()).await;
    assert_eq!(title(&queue).as_deref(), Some("b"));
    let slot = lock(&slot);
    assert_eq!(slot.token, 2);
    assert!(slot.handle.is_some());
}

/// A player whose `Binding` carries the `app` label resolves both frame
/// counters with that label in `play`. A standalone `Call` never mixes
/// (Songbird demotes a mixer without a connection), so the counters stay
/// at zero here; `frames::tests` drives them on Songbird's mock mixer.
#[cfg(feature = "ogg")]
#[tokio::test(flavor = "multi_thread")]
async fn bound_player_labels_its_frame_counters() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let binding = Arc::new(Binding::default());
    let _ = binding.app.set(Arc::from("player-app"));
    let player = SongbirdPlayer::new(call(), media(), Weak::new(), binding);
    metrics::with_local_recorder(&recorder, || player.play(1, &info(good(), 1), 1.0));
    until("the handle", || player.slot().handle.is_some()).await;
    let mut names: Vec<_> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(key, _, _, value)| {
            let key = key.key();
            let labels: Vec<_> = key
                .labels()
                .map(|l| (l.key().to_owned(), l.value().to_owned()))
                .collect();
            assert_eq!(labels, [("app".to_owned(), "player-app".to_owned())]);
            assert!(matches!(value, DebugValue::Counter(_)), "{}", key.name());
            key.name().to_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, [frames::ENCODED_FRAMES, frames::PASSTHROUGH_FRAMES]);
    player.stop(1);
}
