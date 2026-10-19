# Performance

How to make Rust fast where it matters: measure, then remove allocations, indirection and wasted work — without sacrificing clarity elsewhere.

**Contents**
1. Measure first
2. Build profiles
3. Allocation: the usual suspect
4. Strings
5. Small and shared data: `SmallVec`, `Box<str>`, `Arc<str>`
6. Iterators and bounds checks
7. Static vs dynamic dispatch, inlining
8. Data layout and cache friendliness
9. Hash maps and hashers
10. Parallelism with `rayon`
11. I/O
12. Pitfalls

---

## 1. Measure first

Intuition about performance is wrong often enough that optimizing without data mostly
adds complexity. The loop is: **benchmark → profile → change one thing → benchmark**.

- **Always measure release builds.** Debug builds can be 10–100× slower and distort
  everything. `cargo run --release`, `cargo bench`.
- **Microbenchmarks:** `criterion` (statistics, regression detection) or `divan`
  (lighter, attribute-based). Put them in `benches/` with `harness = false`.

```rust
// benches/slugify.rs
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    c.bench_function("slugify", |b| b.iter(|| mycrate::slugify(black_box("Hello, World!"))));
}

criterion_group!(benches, bench);
criterion_main!(benches);
```

`black_box` stops the optimizer from deleting the work you are trying to measure.

- **Profilers:** `cargo flamegraph` (perf/dtrace), `samply`, `perf`, Instruments on
  macOS. Add `debug = "line-tables-only"` (or `true`) to the profile you profile so
  stacks have symbols.
- **Allocation profiling:** `dhat` (heap profiler crate), `heaptrack`.
- **Macro view:** for services, `tracing` spans with timing tell you which *request
  phase* is slow before you zoom into code.

## 2. Build profiles

The default release profile is good. Tune it for shipped binaries once correctness is
settled:

```toml
[profile.release]
lto = "fat"           # cross-crate inlining; slower builds, often 5–20 % faster code
codegen-units = 1     # better optimization, slower builds
panic = "abort"       # smaller/faster binary; no unwinding (catch_unwind won't work)
# strip = true        # smaller binary
# debug = "line-tables-only"  # keep for profiling

[profile.dev.package."*"]
opt-level = 2         # optimize dependencies in dev builds; your own code stays fast to compile
```

`target-cpu=native` (via `RUSTFLAGS` or `.cargo/config.toml`) can help numeric code but
makes binaries non-portable — only for code that runs where it was built. A different
global allocator (`mimalloc`, `jemallocator`) can be a cheap win for allocation-heavy
multithreaded services; measure it.

## 3. Allocation: the usual suspect

Heap allocation is cheap-ish individually, but allocation in a hot loop dominates
profiles surprisingly often.

**Pre-size collections** when you know (or can estimate) the size:

```rust
// ❌ reallocates log₂(n) times
let mut out = Vec::new();
for x in &items { out.push(x.id); }

// ✅ one allocation — and `collect` from an exact-size iterator does this for you
let out: Vec<u64> = items.iter().map(|x| x.id).collect();
let mut buf = String::with_capacity(items.len() * 8);
```

**Reuse buffers** across iterations instead of allocating each time:

```rust
// ❌ a fresh String per line
for line in lines {
    let normalized = line.trim().to_lowercase();
    handle(&normalized);
}

// ✅ one buffer, cleared (capacity kept) each iteration
let mut buf = String::new();
for line in lines {
    buf.clear();
    buf.extend(line.trim().chars().flat_map(char::to_lowercase));
    handle(&buf);
}
```

**Don't collect just to iterate again.** Chain iterators; return `impl Iterator` from
helpers instead of `Vec` when callers only iterate once.

**Clone-on-write** when most inputs pass through unchanged:

```rust
use std::borrow::Cow;

fn escape_html(s: &str) -> Cow<'_, str> {
    if !s.contains(['<', '>', '&']) {
        return Cow::Borrowed(s); // common case: no allocation
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}
```

## 4. Strings

- Take `&str`, return `String` only when you create new text.
- Build strings with `push_str` / `write!` into one buffer, not repeated `format!` +
  `+`. `format!` in a loop allocates every iteration.

```rust
use std::fmt::Write;

// ❌ allocates a temporary String per item, then copies it
let mut s = String::new();
for (k, v) in &pairs { s += &format!("{k}={v};"); }

// ✅ formats straight into the buffer
let mut s = String::new();
for (k, v) in &pairs {
    let _ = write!(s, "{k}={v};"); // writing to a String never fails
}
```

- `to_string()` on a `&str` goes through `Display`; `to_owned()` / `String::from` are
  direct. (Modern std specializes this, but `to_owned` states intent.)
- Compare without allocating: `a.eq_ignore_ascii_case(b)` instead of
  `a.to_lowercase() == b.to_lowercase()`.
- Work on bytes (`as_bytes()`) when the data is known ASCII; `char` iteration decodes
  UTF-8.

## 5. Small and shared data

- **`Box<str>` / `Box<[T]>`** for immutable owned data: 16 bytes instead of 24, no spare
  capacity. Useful for large numbers of stored strings.
- **`Arc<str>`** for strings shared across many owners or threads (interned names,
  keys): cloning is a refcount bump, not a copy.
- **`SmallVec<[T; N]>` / `ArrayVec`** keep up to `N` elements inline, avoiding the heap
  for the common small case. Measure — they add a branch per access and bigger structs.
- **`compact_str` / `smol_str`** for many short strings.
- `Option<Box<T>>`, `Option<&T>`, `Option<NonZeroU32>` are the same size as the inner
  type (niche optimization) — prefer them over sentinel values.

## 6. Iterators and bounds checks

Iterator chains compile to the same (or better) machine code as hand-written loops and
avoid bounds checks, because the iterator already knows the length.

```rust
// ❌ bounds check on every a[i] and b[i] (unless the optimizer proves it away)
let mut dot = 0.0;
for i in 0..a.len() { dot += a[i] * b[i]; }

// ✅ no bounds checks, auto-vectorizes well
let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
```

When you must index, a single up-front assertion or re-slice lets the optimizer drop
the per-iteration checks:

```rust
fn add_into(dst: &mut [u32], a: &[u32], b: &[u32]) {
    let n = dst.len();
    let (a, b) = (&a[..n], &b[..n]); // one length check each, up front…
    for i in 0..n {
        dst[i] = a[i] + b[i];          // …so the optimizer can drop the per-iteration checks
    }
}
```

(An iterator version — `for ((d, x), y) in dst.iter_mut().zip(a).zip(b)` — needs no
re-slicing at all and is what clippy's `needless_range_loop` will suggest.)

Use `chunks_exact` / `windows` / `split_at` for structured access. Reach for
`get_unchecked` only with a benchmark proving it matters and a `// SAFETY:` comment.

## 7. Static vs dynamic dispatch, inlining

- Generics (`impl Trait`, `T: Trait`) are monomorphized: calls are direct and inlinable.
  `dyn Trait` is a vtable call that blocks inlining. In hot inner loops prefer generics;
  at boundaries (plugin lists, heterogeneous collections, reducing compile time/binary
  size) `dyn` is perfectly fine.
- An `enum` over a closed set of implementations is often faster than `Box<dyn Trait>`
  and keeps values inline (crate `enum_dispatch` automates this).
- `#[inline]` matters mainly for small **non-generic** functions called across crate
  boundaries (generic ones are already inlinable). `#[inline(always)]` is rarely right;
  let the compiler and LTO decide unless a profile says otherwise. `#[cold]` on error
  paths can help layout.

## 8. Data layout and cache friendliness

Memory access patterns often matter more than instruction counts.

- **Contiguous beats pointer-chasing.** `Vec<T>` beats `LinkedList<T>` and
  `Vec<Box<T>>` for almost everything.
- **Struct of arrays** when hot loops touch only some fields of many items:
  `struct Particles { xs: Vec<f32>, ys: Vec<f32> }` instead of `Vec<Particle>` with ten
  fields.
- **Keep hot structs small.** Box rarely used large fields; check sizes with
  `std::mem::size_of::<T>()` and a test `assert!(size_of::<Event>() <= 32)`. A huge
  enum variant bloats every value (clippy's `large_enum_variant` catches this).
- **Indices instead of references** for graphs/arenas: `Vec<Node>` + `usize` ids is
  faster and avoids lifetime and `Rc<RefCell>` tangles (`slotmap`, `typed-arena`).

## 9. Hash maps and hashers

`std::collections::HashMap` uses SipHash: DoS-resistant, but slow for small keys.

| Hasher | Speed | Use when |
| --- | --- | --- |
| std default (SipHash) | baseline | keys come from untrusted input (HTTP headers, user ids from the network) |
| `foldhash` / `ahash` | fast, still randomized | general-purpose speedup; reasonable DoS resistance |
| `rustc-hash` (`FxHashMap`) | fastest for ints/small keys | trusted keys only (compiler-style workloads) |

Switching the hasher on attacker-controlled keys can let an attacker force collisions
and turn O(1) into O(n). Also consider whether you need hashing at all: a `Vec` indexed
by a dense id, or a sorted `Vec` with binary search, beats any map for small or dense
key sets. Use `HashMap::with_capacity` and the `entry` API to avoid double lookups.

## 10. Parallelism with `rayon`

For CPU-bound data parallelism, `rayon` turns `iter()` into `par_iter()` with a
work-stealing pool — usually the cheapest big speedup available.

```rust
use rayon::prelude::*;

let total: u64 = files.par_iter().map(|f| count_words(f)).sum();
```

It pays off when each item does real work (microseconds or more); for tiny per-item
work the overhead dominates. Don't run `rayon` work on async runtime threads — use
`tokio::task::spawn_blocking` or a channel to a rayon pool.

## 11. I/O

- Wrap `File`/`TcpStream` in `BufReader` / `BufWriter`; unbuffered small reads/writes
  are syscalls each. `println!` locks stdout every call — in loops, lock once:
  `let mut out = std::io::stdout().lock();` and `writeln!(out, …)`.
- Read whole small files with `fs::read_to_string`; stream large ones line by line.
- `serde_json::from_reader` on an unbuffered file is slow; buffer it or read to a
  `Vec<u8>` and use `from_slice`.

## 12. Pitfalls

- **Benchmarking debug builds** or forgetting `black_box`.
- **Optimizing cold code.** If a profile shows 2 % in a function, a 2× speedup there
  buys 1 %.
- **`clone()` hidden in loops** — of `Vec`, `String`, `HashMap`. Search for it first.
- **`collect()` then `.len()` / `.iter()`** — use `count()` or keep the iterator.
- **`Rc<RefCell<…>>` / `Arc<Mutex<…>>` everywhere** — runtime checks and contention; a
  restructured ownership model is usually both faster and clearer.
- **Lock contention** in async services: a single global `Mutex<HashMap>` serializes all
  requests. Shard, use `DashMap`, or pass messages.
- **Trading clarity for unmeasured gains.** `unsafe` or convoluted code for a speedup
  nobody measured is a net loss.
