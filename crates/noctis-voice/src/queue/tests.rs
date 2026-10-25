//! The queue and idle matrices, on the pure state machine and on a
//! `QueueCell` with a `FakePlayer`.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::fake::{FakePlayer, Played};
use super::*;
use crate::track::{Track, TrackId};

fn info(id: u64) -> TrackInfo {
    Track::file(format!("{id}.wav"))
        .unwrap_or_else(|e| panic!("{e}"))
        .title(format!("t{id}"))
        .into_info(TrackId::new(id))
}

fn ids(infos: &[TrackInfo]) -> Vec<u64> {
    infos.iter().map(|i| i.id().get()).collect()
}

/// The notes as short strings.
fn notes(out: &Out) -> Vec<String> {
    out.notes
        .iter()
        .map(|note| match note {
            Note::Start(t) => format!("start {}", t.id()),
            Note::End(t, r) => format!("end {} {}", t.id(), r.as_str()),
            Note::Error(t, e) => format!("error {} {e}", t.id()),
            Note::Empty => "empty".to_owned(),
        })
        .collect()
}

/// The token of the current track.
fn token(queue: &Queue) -> u64 {
    queue.current.as_ref().map_or(0, |c| c.token)
}

#[test]
fn enqueue_idle_and_busy() {
    let mut queue = Queue::new(1000);
    let mut out = Out::default();
    assert_eq!(queue.enqueue(info(1), &mut out), Ok(()));
    assert!(
        matches!(out.cmds.as_slice(), [Cmd::Play(1, t, v)] if t.id().get() == 1 && (*v - 1.0).abs() < f32::EPSILON)
    );
    assert!(out.notes.is_empty(), "TrackStart waits for playable");
    queue.on_playable(1, &mut out);
    queue.on_playable(1, &mut out);
    assert_eq!(notes(&out), ["start 1"]);
    let mut out = Out::default();
    queue
        .enqueue(info(2), &mut out)
        .unwrap_or_else(|_| panic!());
    queue
        .enqueue(info(3), &mut out)
        .unwrap_or_else(|_| panic!());
    assert!(out.cmds.is_empty() && out.notes.is_empty());
    assert_eq!(ids(&queue.list()), [2, 3]);
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.now_playing().map(|t| t.id().get()), Some(1));
}

#[test]
fn bound() {
    let mut queue = Queue::new(3);
    let mut out = Out::default();
    for id in 1..=2 {
        assert_eq!(queue.enqueue(info(id), &mut out), Ok(()));
    }
    assert_eq!(
        queue.enqueue(info(3), &mut out),
        Ok(()),
        "L == max-1 accepted"
    );
    assert_eq!(queue.enqueue(info(4), &mut out), Err(3), "L == max refused");
    assert_eq!(queue.len(), 3);
    let mut one = Queue::new(1);
    assert_eq!(one.enqueue(info(1), &mut out), Ok(()));
    assert_eq!(one.enqueue(info(2), &mut out), Err(1));
}

#[test]
fn natural_end_error_and_chain() {
    let mut queue = Queue::new(10);
    let mut out = Out::default();
    for id in 1..=2 {
        queue
            .enqueue(info(id), &mut out)
            .unwrap_or_else(|_| panic!());
    }
    let a = token(&queue);
    queue.on_end(a, &mut out);
    let b = token(&queue);
    assert_ne!(a, b);
    queue.on_playable(b, &mut out);
    queue.on_end(a, &mut out); // late: ignored
    queue.on_end(b, &mut out);
    assert_eq!(
        notes(&out),
        ["end 1 finished", "start 2", "end 2 finished", "empty"]
    );
    assert_eq!(out.counted, ["finished", "finished"]);
    assert!(queue.now_playing().is_none());

    let mut out = Out::default();
    for id in 3..=5 {
        queue
            .enqueue(info(id), &mut out)
            .unwrap_or_else(|_| panic!());
    }
    for _ in 0..3 {
        let t = token(&queue);
        queue.on_error(t, "boom".into(), &mut out);
        queue.on_end(t, &mut out); // Songbird's End after Error: ignored
    }
    assert_eq!(
        notes(&out),
        ["error 3 boom", "error 4 boom", "error 5 boom", "empty"]
    );
    assert_eq!(out.counted, ["error", "error", "error"]);
}

#[test]
fn skip_rows() {
    let mut queue = Queue::new(10);
    let mut out = Out::default();
    assert_eq!(queue.skip(&mut out), None);
    assert!(out.notes.is_empty() && out.cmds.is_empty());
    queue
        .enqueue(info(1), &mut out)
        .unwrap_or_else(|_| panic!());
    queue
        .enqueue(info(2), &mut out)
        .unwrap_or_else(|_| panic!());
    assert!(queue.pause(&mut out));
    let a = token(&queue);
    let mut out = Out::default();
    assert_eq!(queue.skip(&mut out).map(|t| t.id().get()), Some(1));
    let b = token(&queue);
    assert_eq!(out.cmds[0], Cmd::Stop(a));
    assert!(matches!(&out.cmds[1], Cmd::Play(t, ..) if *t == b));
    assert!(!queue.is_paused(), "the next track plays");
    queue.on_end(a, &mut out); // late driver End: ignored
    assert_eq!(queue.skip(&mut out).map(|t| t.id().get()), Some(2));
    assert_eq!(notes(&out), ["end 1 skipped", "end 2 skipped", "empty"]);
    assert_eq!(out.counted, ["skipped", "skipped"]);
}

#[test]
fn pause_resume_rows() {
    let mut queue = Queue::new(10);
    let mut out = Out::default();
    assert!(!queue.pause(&mut out) && !queue.resume(&mut out));
    queue
        .enqueue(info(1), &mut out)
        .unwrap_or_else(|_| panic!());
    assert!(queue.pause(&mut out));
    assert!(!queue.pause(&mut out));
    assert!(queue.is_paused());
    queue
        .enqueue(info(2), &mut out)
        .unwrap_or_else(|_| panic!());
    assert!(queue.is_paused(), "paused enqueue keeps the pause");
    assert!(queue.resume(&mut out));
    assert!(!queue.resume(&mut out));
    assert!(!queue.is_paused());
}

#[test]
fn stop_clear_volume() {
    let mut queue = Queue::new(10);
    let mut out = Out::default();
    assert_eq!(queue.stop(&mut out), 0);
    assert!(out.notes.is_empty());
    for id in 1..=3 {
        queue
            .enqueue(info(id), &mut out)
            .unwrap_or_else(|_| panic!());
    }
    assert_eq!(queue.clear(), 2);
    assert_eq!(queue.now_playing().map(|t| t.id().get()), Some(1));
    for id in 4..=5 {
        queue
            .enqueue(info(id), &mut out)
            .unwrap_or_else(|_| panic!());
    }
    let mut out = Out::default();
    assert_eq!(queue.stop(&mut out), 3);
    assert_eq!(notes(&out), ["end 1 stopped", "empty"]);
    assert_eq!(out.counted, ["stopped"]);
    assert!(queue.list().is_empty() && queue.now_playing().is_none());

    assert!((queue.volume() - 1.0).abs() < f32::EPSILON);
    for v in [0.0, 1.0, 2.0] {
        assert_eq!(queue.set_volume(v, &mut out), Ok(()));
    }
    for bad in [-0.01, 2.0001, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(queue.set_volume(bad, &mut out).is_err(), "{bad}");
    }
    assert!((queue.volume() - 2.0).abs() < f32::EPSILON);
    let mut out = Out::default();
    queue
        .enqueue(info(6), &mut out)
        .unwrap_or_else(|_| panic!());
    assert!(matches!(&out.cmds[0], Cmd::Play(_, _, v) if (*v - 2.0).abs() < f32::EPSILON));
    queue.set_volume(0.5, &mut out).unwrap_or_else(|_| panic!());
    assert!(
        matches!(out.cmds.last(), Some(Cmd::SetVolume(_, v)) if (*v - 0.5).abs() < f32::EPSILON)
    );
}

#[test]
fn shutdown_counts_stopped_without_events() {
    let mut queue = Queue::new(10);
    let mut out = Out::default();
    for id in 1..=2 {
        queue
            .enqueue(info(id), &mut out)
            .unwrap_or_else(|_| panic!());
    }
    let mut out = Out::default();
    queue.shutdown(&mut out);
    assert!(out.notes.is_empty());
    assert_eq!(out.counted, ["stopped"]);
    assert_eq!(queue.len(), 0);
}

fn cell(max: usize, idle: Option<(Duration, IdleLeave)>) -> (Arc<QueueCell>, Played) {
    let played = Played::default();
    let cell = QueueCell::new(
        Id::new(9),
        max,
        Box::new(FakePlayer(played.clone())),
        Arc::default(),
        idle,
    );
    (Arc::new(cell), played)
}

#[tokio::test]
async fn cell_runs_commands_and_closes() {
    let (cell, played) = cell(2, None);
    cell.enqueue(info(1)).unwrap_or_else(|e| panic!("{e}"));
    cell.enqueue(info(2)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        cell.enqueue(info(3)),
        Err(VoiceError::QueueFull {
            guild: Id::new(9),
            max: 2
        })
    );
    assert_eq!(
        cell.set_volume(3.0),
        Err(VoiceError::InvalidVolume { volume: 3.0 })
    );
    let first = played.last_token().unwrap_or_default();
    cell.on_end(first);
    assert_eq!(played.titles(), ["t1", "t2"]);
    cell.shutdown();
    assert!(matches!(played.all().last(), Some(Cmd::Stop(_))));
    let interrupted = VoiceError::Interrupted { guild: Id::new(9) };
    assert_eq!(cell.skip(), Err(interrupted.clone()));
    assert_eq!(cell.pause(), Err(interrupted.clone()));
    assert_eq!(cell.enqueue(info(4)), Err(interrupted));
    assert!(cell.read(Queue::len).is_none());
}

/// Idle leave: arms only once connected and idle, is cancelled by a track
/// starting or resuming, re-arms at the next idle and fires after `d`.
#[tokio::test(start_paused = true)]
async fn idle_timer() {
    let fired = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fired);
    let leave: IdleLeave = Arc::new(move || {
        let counter = Arc::clone(&counter);
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
        })
    });
    let after = Duration::from_secs(10);
    let (cell, played) = cell(10, Some((after, leave)));
    // Pending (not connected): no timer.
    assert!(!cell.idle_armed());
    tokio::time::sleep(after * 2).await;
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    cell.set_connected();
    assert!(cell.idle_armed());
    tokio::time::sleep(after / 2).await;
    cell.enqueue(info(1)).unwrap_or_else(|e| panic!("{e}"));
    assert!(!cell.idle_armed(), "a track started");
    cell.pause().unwrap_or_else(|e| panic!("{e}"));
    assert!(cell.idle_armed(), "paused is idle");
    tokio::time::sleep(after / 2).await;
    cell.resume().unwrap_or_else(|e| panic!("{e}"));
    assert!(!cell.idle_armed(), "resumed");
    tokio::time::sleep(after * 2).await;
    assert_eq!(fired.load(Ordering::SeqCst), 0);
    cell.on_end(played.last_token().unwrap_or_default());
    assert!(cell.idle_armed(), "queue empty");
    tokio::time::sleep(after + Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(fired.load(Ordering::SeqCst), 1);
}

/// Without `idle_leave` no timer is ever armed.
#[tokio::test]
async fn no_idle_leave() {
    let (cell, _) = cell(10, None);
    cell.set_connected();
    assert!(!cell.idle_armed());
}
