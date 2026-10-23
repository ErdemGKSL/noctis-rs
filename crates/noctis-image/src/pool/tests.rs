//! The render pool: admission, panic capture, the slot guard, thread names
//! and process scope.

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
fn gate() -> (mpsc::Sender<()>, Arc<std::sync::Mutex<mpsc::Receiver<()>>>) {
    let (tx, rx) = mpsc::channel();
    (tx, Arc::new(std::sync::Mutex::new(rx)))
}

#[tokio::test]
async fn full_queue_fails_at_once_and_admitted_jobs_finish() {
    let pool = pool(1, 2);
    let (open, gate) = gate();
    let blocked = |n: u32| {
        let gate = Arc::clone(&gate);
        move || {
            gate.lock().unwrap().recv().unwrap();
            Ok(n)
        }
    };
    let a = pool.try_submit(blocked(1)).unwrap();
    let b = pool.try_submit(blocked(2)).unwrap();
    assert!(matches!(
        pool.try_submit(|| Ok(3)),
        Err(RenderError::QueueFull)
    ));
    open.send(()).unwrap();
    open.send(()).unwrap();
    assert_eq!(a.await.unwrap().unwrap(), 1);
    assert_eq!(b.await.unwrap().unwrap(), 2);
    // Both slots were released before the results were sent.
    let d = pool.try_submit(|| Ok(4)).unwrap();
    assert_eq!(d.await.unwrap().unwrap(), 4);
}

#[tokio::test]
async fn a_panic_is_an_error_and_frees_its_slot() {
    let pool = pool(1, 1);
    let boom = pool
        .try_submit(|| -> Result<(), RenderError> { panic!("boom") })
        .unwrap();
    assert!(matches!(
        boom.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "boom"
    ));
    let formatted = pool
        .try_submit(|| -> Result<(), RenderError> { panic!("{}", "fmt") })
        .unwrap();
    assert!(matches!(
        formatted.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "fmt"
    ));
    let other = pool
        .try_submit(|| -> Result<(), RenderError> { std::panic::panic_any(5_u8) })
        .unwrap();
    assert!(matches!(
        other.await.unwrap(),
        Err(RenderError::Panicked { message }) if message == "<non-string panic>"
    ));
    let next = pool.try_submit(|| Ok(7)).unwrap();
    assert_eq!(next.await.unwrap().unwrap(), 7);
}

#[tokio::test]
async fn jobs_run_on_named_pool_threads_outside_tokio() {
    let pool = pool(2, 4);
    let rx = pool
        .try_submit(|| {
            let name = std::thread::current().name().map(str::to_owned);
            Ok((name, tokio::runtime::Handle::try_current().is_err()))
        })
        .unwrap();
    let (name, no_tokio) = rx.await.unwrap().unwrap();
    assert!(name.unwrap().starts_with("noctis-render-"));
    assert!(no_tokio);
}

#[tokio::test]
async fn a_dropped_receiver_is_ignored() {
    let pool = pool(1, 1);
    let (done_tx, done_rx) = mpsc::channel();
    drop(
        pool.try_submit(move || {
            done_tx.send(()).unwrap();
            Ok(())
        })
        .unwrap(),
    );
    done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // The slot frees once the job ends, even though nobody waits for it.
    let rx = loop {
        match pool.try_submit(|| Ok(1)) {
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
