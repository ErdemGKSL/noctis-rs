# Async and Concurrency

How to write concurrent Rust that is correct, doesn't stall the runtime, and shuts down cleanly — with `tokio` for I/O, threads/`rayon` for CPU work.

## Contents

1. [Async or threads?](#1-async-or-threads)
2. [Tokio basics: runtime, spawn, JoinSet](#2-tokio-basics)
3. [Never block the runtime](#3-never-block-the-runtime)
4. [Timeouts and `select!`](#4-timeouts-and-select)
5. [Shared state: which `Mutex`, and guards across `.await`](#5-shared-state)
6. [Message passing: channels and the actor pattern](#6-message-passing)
7. [Cancellation and graceful shutdown](#7-cancellation-and-graceful-shutdown)
8. [`Send + 'static` and spawned futures](#8-send--static-and-spawned-futures)
9. [Async functions in traits](#9-async-functions-in-traits)
10. [CPU parallelism: rayon and scoped threads](#10-cpu-parallelism)
11. [Atomics](#11-atomics)
12. [Deadlocks](#12-deadlocks)
13. [Observability: `tracing`](#13-observability-tracing)
14. [Pitfalls](#14-pitfalls)

---

## 1. Async or threads?

| Workload | Use |
| --- | --- |
| many concurrent I/O operations (HTTP server, bot, websocket, DB clients) | async (`tokio`) |
| CPU-bound data processing (parse, compress, hash, image work) | `rayon` or `std::thread` |
| a few long-lived background workers, simple CLI | plain threads + channels are fine |
| mixed | async for I/O, `spawn_blocking` / rayon for CPU chunks |

**Why:** async multiplexes thousands of *waiting* tasks onto a few threads; it gives no speedup for computation and makes code more complex. Don't make a program async just because a dependency could be.

## 2. Tokio basics

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let handle = tokio::spawn(async { fetch_count().await });
    let count = handle.await??; // outer ?: JoinError (panic/cancel), inner ?: task's own error
    println!("{count}");
    Ok(())
}
```

Running a dynamic number of tasks? Use `JoinSet` instead of a `Vec<JoinHandle>`: it yields results as they finish and aborts remaining tasks when dropped.

```rust
use tokio::task::JoinSet;

async fn fetch_all(urls: Vec<String>) -> Vec<Result<usize, Error>> {
    let mut set = JoinSet::new();
    for url in urls {
        set.spawn(async move { fetch_len(&url).await });
    }
    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res.expect("task panicked"));
    }
    results
}
```

To bound concurrency (don't open 10 000 sockets at once), use a `Semaphore`, or `futures::stream::iter(..).buffer_unordered(n)`:

```rust
use futures::stream::{self, StreamExt};

let lens: Vec<_> = stream::iter(urls)
    .map(|url| async move { fetch_len(&url).await })
    .buffer_unordered(16)
    .collect()
    .await;
```

For concurrently awaiting a fixed set of futures *in the same task*, use `tokio::join!` / `tokio::try_join!` — no spawning, no `'static` requirement.

Pick runtime flavor deliberately: the default multi-thread runtime for servers; `#[tokio::main(flavor = "current_thread")]` for small tools and tests. Enable only the tokio features you need (`rt-multi-thread`, `macros`, `net`, `time`, `sync`, `signal`, …).

## 3. Never block the runtime

**Why:** tokio runs many tasks per worker thread and switches only at `.await`. A blocking call (sync file/network I/O, `std::thread::sleep`, heavy CPU loop, a sync DB driver) freezes every other task on that thread — latency spikes, timeouts, apparent deadlocks.

```rust
// ❌ blocks a worker thread
async fn handler() -> String {
    std::thread::sleep(Duration::from_secs(1));
    std::fs::read_to_string("big.txt").unwrap()
}

// ✅ async equivalents
async fn handler() -> std::io::Result<String> {
    tokio::time::sleep(Duration::from_secs(1)).await;
    tokio::fs::read_to_string("big.txt").await
}

// ✅ CPU-heavy or sync-only code goes to the blocking pool
async fn hash_password(pw: String) -> anyhow::Result<String> {
    let hash = tokio::task::spawn_blocking(move || expensive_hash(&pw)).await?;
    Ok(hash)
}
```

Rough rule: if a piece of code may run more than ~10–100 µs without hitting an `.await`, move it off the async threads. For sustained CPU parallelism, send work to rayon and await a `oneshot` reply.

## 4. Timeouts and `select!`

Every network operation should have a timeout; remote peers can hang forever.

```rust
use tokio::time::{timeout, Duration};

let body = timeout(Duration::from_secs(10), fetch(url)).await
    .map_err(|_| Error::Timeout)??;
```

`tokio::select!` races several futures and drops the losers:

```rust
loop {
    tokio::select! {
        Some(msg) = rx.recv() => handle(msg).await,
        _ = interval.tick() => flush().await,
        _ = token.cancelled() => break,
    }
}
```

Pitfall: dropping a losing branch *cancels* it mid-way. See cancellation safety in §7.

## 5. Shared state

### Which mutex?

| Situation | Use |
| --- | --- |
| short critical section, no `.await` while locked (the common case) | `std::sync::Mutex` (or `parking_lot::Mutex`) |
| you must hold the lock across an `.await` (e.g. a shared connection used by one task at a time) | `tokio::sync::Mutex` |
| read-mostly data | `RwLock` (std or tokio), or swap whole snapshots with `arc-swap` |
| concurrent map | `dashmap`, or sharding, or an actor |

The tokio docs themselves recommend `std::sync::Mutex` for plain data: it is faster, and the compiler stops you from holding it across `.await` in a spawned task (its guard is `!Send`).

### Don't hold guards across `.await`

```rust
// ❌ guard alive across .await: blocks others, and makes the future !Send
async fn bump(state: &Mutex<State>) {
    let mut s = state.lock().unwrap();
    s.count += 1;
    save(&s).await;
}

// ✅ scope the lock, copy out what you need, then await
async fn bump(state: &Mutex<State>) {
    let snapshot = {
        let mut s = state.lock().unwrap();
        s.count += 1;
        s.clone()
    }; // guard dropped here
    save(&snapshot).await;
}
```

Share state as `Arc<T>` where `T` holds the locks internally; wrap it in a cheap-to-clone handle type rather than passing `Arc<Mutex<…>>` everywhere:

```rust
#[derive(Clone, Default)]
pub struct AppState {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: Mutex<HashMap<UserId, Session>>,
    hits: AtomicU64,
}

impl AppState {
    pub fn record_hit(&self) { self.inner.hits.fetch_add(1, Ordering::Relaxed); }
    pub fn session(&self, id: UserId) -> Option<Session> {
        self.inner.sessions.lock().unwrap().get(&id).cloned()
    }
}
```

The handle's methods keep lock scopes short and invisible to callers. `lock().unwrap()` is acceptable: it panics only if another thread panicked while holding the lock (poisoning), which is already a bug. `parking_lot` mutexes don't poison.

## 6. Message passing

Often clearer than shared locks: one task owns the data, others send it messages.

| Channel | Shape | Use for |
| --- | --- | --- |
| `tokio::sync::mpsc` | many → one, bounded | work queues, commands to an actor |
| `tokio::sync::oneshot` | one value, once | replies to a request |
| `tokio::sync::broadcast` | many → many, every receiver gets every message | events, chat fan-out |
| `tokio::sync::watch` | latest value only | config reloads, shutdown/status flags |
| `std::sync::mpsc` / `crossbeam-channel` / `flume` | thread channels | non-async workers |

Prefer **bounded** channels: they apply backpressure instead of growing memory without limit when consumers fall behind.

### Actor pattern

```rust
use tokio::sync::{mpsc, oneshot};

enum Command {
    Get { key: String, reply: oneshot::Sender<Option<String>> },
    Set { key: String, value: String },
}

#[derive(Clone)]
pub struct KvHandle { tx: mpsc::Sender<Command> }

impl KvHandle {
    pub fn spawn() -> Self {
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut store = HashMap::new(); // owned by exactly one task — no locks
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::Get { key, reply } => { let _ = reply.send(store.get(&key).cloned()); }
                    Command::Set { key, value } => { store.insert(key, value); }
                }
            }
            // all handles dropped → loop ends → actor exits
        });
        Self { tx }
    }

    pub async fn get(&self, key: impl Into<String>) -> Option<String> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(Command::Get { key: key.into(), reply }).await.ok()?;
        rx.await.ok()?
    }

    pub async fn set(&self, key: impl Into<String>, value: impl Into<String>) {
        let _ = self.tx.send(Command::Set { key: key.into(), value: value.into() }).await;
    }
}
```

The command `enum` makes the actor's protocol explicit and exhaustively matched.

## 7. Cancellation and graceful shutdown

In async Rust, **cancellation = dropping a future**. Any `.await` point may be the last one executed (because of `select!`, `timeout`, `JoinSet` drop, `abort()`). Code is *cancel-safe* if being dropped at an `.await` loses no data.

- `mpsc::Receiver::recv`, `tokio::time::sleep`, `TcpListener::accept` are cancel-safe — fine in a `select!` loop.
- `AsyncReadExt::read_exact`, `read_to_end`, or your own function that reads half a message and then awaits are **not**: partial data is lost if the branch loses. Create such futures once outside the loop and `select!` on `&mut fut`, or move them into a dedicated task.
- Don't put "must finish" work (DB transaction commit, writing a file) where it can be cancelled midway; spawn it or make it idempotent.

### Graceful shutdown with `CancellationToken`

```rust
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let token = CancellationToken::new();
    let tracker = TaskTracker::new();

    for id in 0..4 {
        let token = token.child_token();
        tracker.spawn(async move { worker(id, token).await });
    }

    tokio::signal::ctrl_c().await?;
    tracing::info!("shutting down");
    token.cancel();          // tell everyone to stop
    tracker.close();
    tracker.wait().await;    // wait for them to finish cleanly
    Ok(())
}

async fn worker(id: u32, token: CancellationToken) {
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => tracing::debug!(id, "tick"),
        }
    }
    // cleanup here: flush buffers, close connections
}
```

Prefer cooperative cancellation (token checked at safe points) over `JoinHandle::abort()`, which can stop a task in the middle of anything.

## 8. `Send + 'static` and spawned futures

`tokio::spawn` requires the future to be `Send + 'static` because it may run on any thread and outlive the caller.

- **`'static`**: the task can't borrow locals. Move owned data in (`async move`), clone an `Arc`, or use `join!`/`JoinSet` with owned values. Clone *before* the `async move` block:

```rust
let state = state.clone();
tokio::spawn(async move { state.record_hit(); });
```

- **`Send`**: everything alive across an `.await` must be `Send`. Common culprits: `Rc`, `RefCell`, `std::sync::MutexGuard`, `*mut T`, a `Box<dyn Error>` without `+ Send + Sync`. The compiler error points at the `.await` where the non-`Send` value is held — drop it before that point (use a block scope), or switch to `Arc`/`Mutex`.
- For errors crossing tasks, use `Box<dyn Error + Send + Sync>` / `anyhow::Error` (which is `Send + Sync`).
- Genuinely `!Send` work (some GUI/FFI libraries) can run on a `LocalSet` / `current_thread` runtime with `spawn_local`.

## 9. Async functions in traits

Since Rust 1.75, `async fn` works directly in traits — no `async-trait` crate needed for static dispatch:

```rust
pub trait Storage {
    async fn load(&self, id: u64) -> anyhow::Result<Vec<u8>>;
}

pub struct MemStorage;
impl Storage for MemStorage {
    async fn load(&self, _id: u64) -> anyhow::Result<Vec<u8>> { Ok(vec![]) }
}

async fn run(storage: &impl Storage) -> anyhow::Result<()> {
    let _bytes = storage.load(1).await?;
    Ok(())
}
```

Limitations:
- Such traits are **not dyn-compatible**: `Box<dyn Storage>` doesn't work. If you need dynamic dispatch, use the `async-trait` crate (boxes each future) or write `fn load(&self, id: u64) -> Pin<Box<dyn Future<Output = …> + Send + '_>>` by hand.
- Callers can't require the returned future to be `Send` in generic code — rustc's `async_fn_in_trait` lint warns about exactly this on `pub` traits (it's fine to `#[allow]` it for crate-internal traits). For public traits used with `tokio::spawn`, declare the method as `fn load(&self, id: u64) -> impl Future<Output = anyhow::Result<Vec<u8>>> + Send;` (implementors can still write `async fn`) — or use the `trait-variant` crate to generate a `Send` variant.

## 10. CPU parallelism

### rayon

Turning a sequential iterator into a parallel one is often a one-word change:

```rust
use rayon::prelude::*;

let total: u64 = files.par_iter().map(|f| count_words(f)).sum();
images.par_iter_mut().for_each(|img| img.blur(2.0));
```

**Why:** rayon's work-stealing pool balances uneven work automatically and borrows data safely (no `'static` needed). Worth it when each item does real work — for tiny per-item work, the overhead dominates. Don't call rayon's blocking APIs directly on tokio worker threads; wrap them in `spawn_blocking` or hand results back over a `oneshot`.

### Scoped threads

`std::thread::scope` lets threads borrow from the stack — no `Arc`, no `'static`:

```rust
let mut left = vec![0u64; n];
let mut right = vec![0u64; n];
std::thread::scope(|s| {
    s.spawn(|| fill(&mut left));
    s.spawn(|| fill(&mut right));
}); // all threads joined here; borrows end
```

## 11. Atomics

For counters and flags, atomics are cheaper than a mutex:

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static REQUESTS: AtomicU64 = AtomicU64::new(0);
REQUESTS.fetch_add(1, Ordering::Relaxed); // independent counter: Relaxed is enough

// publishing data to another thread: Release on store, Acquire on load
READY.store(true, Ordering::Release);
if READY.load(Ordering::Acquire) { /* sees writes made before the store */ }
```

`Relaxed` for statistics; `Release`/`Acquire` pairs when the flag guards other data; `SeqCst` if unsure (correct, slightly slower). Anything involving two related values belongs in a `Mutex`, not two atomics. Use `OnceLock`/`LazyLock` for one-time initialization instead of hand-rolled flag logic.

## 12. Deadlocks

- Acquire multiple locks in a **consistent global order**, or better, never hold two at once.
- Never call unknown code (callbacks, trait objects, `.await` on someone else's future) while holding a lock.
- `RwLock` read guards can deadlock if a writer is queued and you take a second read lock in the same thread; don't re-lock.
- Bounded channels can deadlock in cycles (A waits to send to B while B waits to send to A); break cycles with `try_send`, unbounded control channels, or a different topology.
- `std::sync::Mutex` is not re-entrant: locking twice in the same thread deadlocks (or panics).

## 13. Observability: `tracing`

Use `tracing` instead of `println!`/`eprintln!` in services. It gives levels, structured fields and spans that follow a request across `.await` points and tasks.

```rust
use tracing::{info, instrument};

#[instrument(skip(db))] // records `id` via Debug; skipped args needn't be Debug
async fn load_user(db: &Db, id: UserId) -> anyhow::Result<User> {
    let user = db.fetch(id).await?;
    info!(name = %user.name, "loaded user");
    Ok(user)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()) // RUST_LOG=info,my_crate=debug
        .init();
}
```

`#[instrument]` records every argument it doesn't `skip` using `Debug` — `skip` large, secret or non-`Debug` arguments (or `skip_all` and list `fields(...)` explicitly). For spawned tasks, carry the span with `.instrument(span)` from `tracing::Instrument`. `tokio-console` helps find tasks that never yield.

## 14. Pitfalls

- **Forgetting `.await`** — a future does nothing until awaited (the `unused_must_use` warning catches most cases).
- **`block_on` inside async code** panics ("cannot start a runtime from within a runtime") or deadlocks. Stay async all the way up, or use `spawn_blocking`.
- **Creating a new `reqwest::Client` / DB pool per request** — clients hold connection pools; create once and share (they're cheap to clone).
- **Unbounded channels and unbounded `spawn` loops** turn load spikes into out-of-memory crashes.
- **Detached tasks swallowing errors** — a `JoinHandle` you never await hides panics and errors; log inside the task or track it in a `JoinSet`/`TaskTracker`.
- **`tokio::sync::Mutex` by default** — slower and still serializes; use it only when the lock genuinely spans `.await`.
- **Spinning on `try_recv` / `loop {}` with no await** starves the runtime; await the channel instead.
- **`Arc<Mutex<Vec<T>>>` as a work queue** — use a channel.
