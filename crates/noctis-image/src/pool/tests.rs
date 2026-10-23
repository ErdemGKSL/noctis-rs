//! The render pool: weighted admission, the still reserve and lane, skipping
//! a gone requester, panic capture, the release guard, thread names and
//! process scope.

use std::sync::mpsc;
use std::time::Duration;

use noctis_core::Runtime;

use super::*;

fn pool(threads: usize, queue_capacity: usize) -> Arc<RenderPool> {
    Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads,
            queue_capacity,
        })
        .unwrap()
}

/// A gate the test opens; jobs block on it.
fn gate() -> (mpsc::Sender<()>, Arc<Mutex<mpsc::Receiver<()>>>) {
    let (tx, rx) = mpsc::channel();
    (tx, Arc::new(Mutex::new(rx)))
}

#[tokio::test]
async fn full_queue_fails_at_once_and_admitted_jobs_finish() {
    let pool = pool(1, 2);
    let (open, gate) = gate();
    let blocked = |n: u32| {
        let gate = Arc::clone(&gate);
        move |_: &Requester<'_>| {
            gate.lock().unwrap().recv().unwrap();
            Ok(n)
        }
    };
    let a = pool.try_submit(Lane::Still, blocked(1)).unwrap();
    let b = pool.try_submit(Lane::Still, blocked(2)).unwrap();
    assert!(matches!(
        pool.try_submit(Lane::Still, |_| Ok(3)),
        Err(RenderError::QueueFull)
    ));
    open.send(()).unwrap();
    open.send(()).unwrap();
    assert_eq!(a.await.unwrap().unwrap(), 1);
    assert_eq!(b.await.unwrap().unwrap(), 2);
    // Both slots were released before the results were sent.
    let d = pool.try_submit(Lane::Still, |_| Ok(4)).unwrap();
    assert_eq!(d.await.unwrap().unwrap(), 4);
}

#[tokio::test]
async fn a_panic_is_an_error_and_frees_its_slot() {
    let pool = pool(1, 1);
    let boom = pool
        .try_submit(Lane::Still, |_| -> Result<(), RenderError> {
            panic!("boom")
        })
        .unwrap();
    assert!(matches!(
        boom.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "boom"
    ));
    let formatted = pool
        .try_submit(Lane::Still, |_| -> Result<(), RenderError> {
            panic!("{}", "fmt")
        })
        .unwrap();
    assert!(matches!(
        formatted.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "fmt"
    ));
    let other = pool
        .try_submit(Lane::Still, |_| -> Result<(), RenderError> {
            std::panic::panic_any(5_u8)
        })
        .unwrap();
    assert!(matches!(
        other.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "<non-string panic>"
    ));
    let next = pool.try_submit(Lane::Still, |_| Ok(7)).unwrap();
    assert_eq!(next.await.unwrap().unwrap(), 7);
}

#[tokio::test]
async fn jobs_run_on_named_pool_threads_outside_tokio() {
    let pool = pool(2, 4);
    let rx = pool
        .try_submit(Lane::Still, |_| {
            let name = std::thread::current().name().map(str::to_owned);
            Ok((name, tokio::runtime::Handle::try_current().is_err()))
        })
        .unwrap();
    let (name, no_tokio) = rx.await.unwrap().unwrap();
    assert!(name.unwrap().starts_with("noctis-render-"));
    assert!(no_tokio);
}

#[tokio::test]
async fn a_receiver_dropped_while_running_is_ignored() {
    let pool = pool(1, 1);
    let (open, gate) = gate();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let rx = pool
        .try_submit(Lane::Still, move |_| {
            started_tx.send(()).unwrap();
            gate.lock().unwrap().recv().unwrap();
            done_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(rx);
    open.send(()).unwrap();
    done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // The slot frees once the job ends, even though nobody waits for it.
    let rx = loop {
        match pool.try_submit(Lane::Still, |_| Ok(1)) {
            Ok(rx) => break rx,
            Err(RenderError::QueueFull) => tokio::task::yield_now().await,
            Err(other) => panic!("{other}"),
        }
    };
    assert_eq!(rx.await.unwrap().unwrap(), 1);
}

#[test]
fn equal_configs_share_one_pool_and_others_conflict() {
    let runtime = Runtime::for_test();
    let config = PoolConfig {
        threads: 1,
        queue_capacity: 8,
    };
    let a = runtime.resource_or_init::<RenderPool>(config).unwrap();
    let b = runtime.resource_or_init::<RenderPool>(config).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    let conflict = runtime
        .resource_or_init::<RenderPool>(PoolConfig {
            threads: 2,
            ..config
        })
        .unwrap_err();
    assert!(conflict.resource.ends_with("RenderPool"), "{conflict}");
    // Another test Runtime has its own pool.
    let other = Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads: 2,
            ..config
        })
        .unwrap();
    assert!(!Arc::ptr_eq(&a, &other));
    assert_eq!(a.build_error(), None);
}

#[test]
fn the_reserve_is_a_floored_quarter() {
    for (capacity, expected) in [(1, 0), (3, 0), (4, 1), (64, 16), (4096, 1024)] {
        assert_eq!(reserve(capacity), expected, "capacity {capacity}");
    }
}

#[test]
fn units_weigh_frames_per_eight_within_the_share() {
    let anim = |frames| Lane::Animated { frames };
    for (lane, capacity, expected) in [
        (Lane::Still, 64, 1),
        (anim(0), 64, 1),
        (anim(1), 64, 1),
        (anim(8), 64, 1),
        (anim(9), 64, 2),
        (anim(90), 64, 12),
        (anim(1000), 64, 48),
        (anim(u64::MAX), 64, 48),
        (anim(90), 8, 6),
        (anim(90), 1, 1),
        (Lane::Still, 1, 1),
    ] {
        assert_eq!(units(lane, capacity), expected, "{lane:?} on {capacity}");
    }
}

/// Submits a job in `lane` that blocks on `gate` until the test opens it.
fn gated(
    pool: &RenderPool,
    lane: Lane,
    gate: &Arc<Mutex<mpsc::Receiver<()>>>,
) -> Result<oneshot::Receiver<Result<(), RenderError>>, RenderError> {
    let gate = Arc::clone(gate);
    pool.try_submit(lane, move |_| {
        gate.lock().unwrap().recv().unwrap();
        Ok(())
    })
}

/// Waits (up to 5 s) until `pool` has no admitted units.
async fn drained(pool: &RenderPool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while pool.admitted() != 0 {
        assert!(std::time::Instant::now() < deadline, "{pool:?}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn animations_keep_to_their_share_and_stills_use_the_reserve() {
    let pool = pool(1, 8);
    let (open, gate) = gate();
    let big = gated(&pool, Lane::Animated { frames: 48 }, &gate).unwrap();
    assert_eq!(pool.admitted(), 6);
    assert!(matches!(
        gated(&pool, Lane::Animated { frames: 1 }, &gate),
        Err(RenderError::QueueFull)
    ));
    let s7 = gated(&pool, Lane::Still, &gate).unwrap();
    let s8 = gated(&pool, Lane::Still, &gate).unwrap();
    assert_eq!(pool.admitted(), 8);
    assert!(matches!(
        gated(&pool, Lane::Still, &gate),
        Err(RenderError::QueueFull)
    ));
    for _ in 0..3 {
        open.send(()).unwrap();
    }
    for rx in [big, s7, s8] {
        rx.await.unwrap().unwrap();
    }
    assert_eq!(pool.admitted(), 0);
}

#[tokio::test]
async fn a_capacity_of_one_reserves_nothing() {
    let pool = pool(1, 1);
    let (open, gate) = gate();
    let anim = gated(&pool, Lane::Animated { frames: 90 }, &gate).unwrap();
    assert!(matches!(
        pool.try_submit(Lane::Still, |_| Ok(())),
        Err(RenderError::QueueFull)
    ));
    open.send(()).unwrap();
    anim.await.unwrap().unwrap();
    let still = pool.try_submit(Lane::Still, |_| Ok(2)).unwrap();
    assert_eq!(still.await.unwrap().unwrap(), 2);
}

#[tokio::test]
async fn a_still_runs_before_animations_queued_earlier() {
    let pool = pool(1, 8);
    let order = Arc::new(Mutex::new(Vec::new()));
    let (open, gate) = gate();
    let (started_tx, started_rx) = mpsc::channel();
    let record = |name: &'static str, block: bool| {
        let order = Arc::clone(&order);
        let gate = Arc::clone(&gate);
        let started_tx = started_tx.clone();
        move |_: &Requester<'_>| {
            order.lock().unwrap().push(name);
            if block {
                started_tx.send(()).unwrap();
                gate.lock().unwrap().recv().unwrap();
            }
            Ok(())
        }
    };
    let one = Lane::Animated { frames: 1 };
    let a0 = pool.try_submit(one, record("A0", true)).unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let rest = [
        pool.try_submit(one, record("A1", false)).unwrap(),
        pool.try_submit(one, record("A2", false)).unwrap(),
        pool.try_submit(one, record("A3", false)).unwrap(),
        pool.try_submit(Lane::Still, record("S", false)).unwrap(),
    ];
    open.send(()).unwrap();
    a0.await.unwrap().unwrap();
    for rx in rest {
        rx.await.unwrap().unwrap();
    }
    assert_eq!(*order.lock().unwrap(), ["A0", "S", "A1", "A2", "A3"]);
}

#[tokio::test]
async fn a_queued_job_whose_requester_is_gone_is_skipped() {
    let pool = pool(1, 4);
    let (open, gate) = gate();
    let a = gated(&pool, Lane::Still, &gate).unwrap();
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let b = pool
        .try_submit(Lane::Animated { frames: 16 }, move |_| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert_eq!(pool.admitted(), 3);
    drop(b);
    open.send(()).unwrap();
    a.await.unwrap().unwrap();
    drained(&pool).await;
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_running_job_sees_its_requester_leave() {
    let pool = pool(1, 4);
    let (open, gate) = gate();
    let (seen_tx, seen_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let rx = pool
        .try_submit(Lane::Still, move |requester| {
            let before = requester.gone();
            started_tx.send(()).unwrap();
            gate.lock().unwrap().recv().unwrap();
            seen_tx.send((before, requester.gone())).unwrap();
            Ok(())
        })
        .unwrap();
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(rx);
    open.send(()).unwrap();
    let seen = seen_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(seen, (false, true));
    drained(&pool).await;
}

#[tokio::test]
async fn a_panicking_animation_frees_its_weight() {
    let pool = pool(1, 8);
    let boom = pool
        .try_submit(
            Lane::Animated { frames: 48 },
            |_| -> Result<(), RenderError> { panic!("boom") },
        )
        .unwrap();
    assert!(matches!(
        boom.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "boom"
    ));
    assert_eq!(pool.admitted(), 0);
    let next = pool
        .try_submit(Lane::Animated { frames: 48 }, |_| Ok(1))
        .unwrap();
    assert_eq!(next.await.unwrap().unwrap(), 1);
}
