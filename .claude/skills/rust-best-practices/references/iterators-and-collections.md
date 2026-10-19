# Iterators and Collections

How to loop, transform and store data idiomatically: iterator chains over index loops, and the right collection for the job.

## Contents

1. [Iterators over index loops](#1-iterators-over-index-loops)
2. [`iter` vs `iter_mut` vs `into_iter`](#2-iter-vs-iter_mut-vs-into_iter)
3. [The adaptor toolbox](#3-the-adaptor-toolbox)
4. [Collecting: `Vec`, `String`, `HashMap`, `Result`, `Option`](#4-collecting)
5. [Laziness and avoiding intermediate `Vec`s](#5-laziness-and-avoiding-intermediate-vecs)
6. [Returning iterators](#6-returning-iterators)
7. [When a `for` loop is clearer](#7-when-a-for-loop-is-clearer)
8. [Choosing a collection](#8-choosing-a-collection)
9. [`HashMap` idioms: entry API and borrowed keys](#9-hashmap-idioms)
10. [`Vec` idioms: capacity, retain, drain, dedup, sort, search](#10-vec-idioms)
11. [Pitfalls](#11-pitfalls)

---

## 1. Iterators over index loops

**Why:** iterators express *what* you compute rather than *how* you walk the indices. They cannot go out of bounds, the optimizer removes bounds checks, and they compose.

```rust
// ❌ index loop: off-by-one risk, bounds checks, noisy
let mut total = 0;
for i in 0..prices.len() {
    if prices[i] > 100 {
        total += prices[i];
    }
}

// ✅
let total: u64 = prices.iter().filter(|&&p| p > 100).sum();
```

Need the index too? Use `enumerate`, not a range:

```rust
for (i, name) in names.iter().enumerate() {
    println!("{i}: {name}");
}
```

Walking two sequences in lockstep? Use `zip` — it stops at the shorter one, so no length mismatch panics:

```rust
let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
```

## 2. `iter` vs `iter_mut` vs `into_iter`

| Call | Yields | Collection afterwards | `for` sugar |
| --- | --- | --- | --- |
| `v.iter()` | `&T` | still usable | `for x in &v` |
| `v.iter_mut()` | `&mut T` | still usable, modified | `for x in &mut v` |
| `v.into_iter()` | `T` (owned) | consumed (moved) | `for x in v` |

Pick the weakest access you need. Consume (`into_iter`) when you are done with the collection and want to move elements out without cloning:

```rust
// ❌ clones every string because the Vec is borrowed
let upper: Vec<String> = words.iter().map(|w| w.clone().to_uppercase()).collect();

// ✅ if `words` is no longer needed, move out of it
let owned: Vec<String> = words.into_iter().filter(|w| !w.is_empty()).collect();
```

`.copied()` / `.cloned()` turn `&T` into `T`; prefer `copied()` for `Copy` types — it documents that the copy is cheap.

## 3. The adaptor toolbox

Know these by heart; most hand-written loops are one of them.

| Goal | Adaptor |
| --- | --- |
| transform each element | `map` |
| keep some elements | `filter` |
| transform + drop `None`s in one pass | `filter_map` |
| each element expands to many | `flat_map` / `flatten` |
| stop at first non-match | `take_while`, `skip_while`, `map_while` |
| first match | `find`, `find_map`, `position` |
| any / all match | `any`, `all` |
| reduce to one value | `sum`, `product`, `min`, `max`, `min_by_key`, `max_by_key`, `fold`, `reduce` |
| fallible reduce, early exit | `try_fold`, `try_for_each` |
| count | `count` |
| pairs | `zip`, `enumerate` |
| concatenate | `chain` |
| sliding / fixed groups on slices | `windows(n)`, `chunks(n)`, `chunks_exact(n)` |
| last / nth | `last`, `nth`, `rev` (double-ended) |
| inspect while debugging | `inspect` |

Examples:

```rust
// filter_map: parse what parses, skip the rest
let ids: Vec<u32> = raw.iter().filter_map(|s| s.parse().ok()).collect();

// find_map: first element that yields Some
let port = args.iter().find_map(|a| a.strip_prefix("--port="));

// flat_map: every user has many roles
let all_roles: Vec<&str> = users.iter().flat_map(|u| u.roles.iter().map(String::as_str)).collect();

// windows: detect consecutive duplicates in a slice
let has_repeat = readings.windows(2).any(|w| w[0] == w[1]);

// max_by_key: no manual "best so far" bookkeeping
let oldest = users.iter().max_by_key(|u| u.age);

// try_fold: sum with overflow check, stop at first failure
let total: Option<u32> = values.iter().try_fold(0u32, |acc, &x| acc.checked_add(x));
```

`position` returns the index; `find` returns the element. `any`/`all` short-circuit — don't use `filter(..).count() > 0`.

## 4. Collecting

`collect` builds anything that implements `FromIterator`. Annotate the target type once, either on the binding or with turbofish.

```rust
let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
let csv: String = names.join(",");                      // join for separators
let initials: String = names.iter().filter_map(|n| n.chars().next()).collect();
let by_id: HashMap<u32, &User> = users.iter().map(|u| (u.id, u)).collect();
let unique: HashSet<&str> = names.iter().copied().collect();
```

**Collecting into `Result` / `Option`** short-circuits on the first error — this is the idiomatic way to "parse all or fail":

```rust
// ❌ manual loop with early return
let mut nums = Vec::new();
for s in &input {
    match s.parse::<i64>() {
        Ok(n) => nums.push(n),
        Err(e) => return Err(e),
    }
}

// ✅ Result<Vec<_>, _> stops at the first Err
let nums: Result<Vec<i64>, _> = input.iter().map(|s| s.parse::<i64>()).collect();
let nums = nums?;
```

The same works for `Option<Vec<T>>`, and for `sum`/`product` into `Result<T, E>`/`Option<T>`.

To split into two collections in one pass, use `partition`:

```rust
let (adults, minors): (Vec<&User>, Vec<&User>) = users.iter().partition(|u| u.age >= 18);
```

`unzip` turns an iterator of pairs into two collections.

## 5. Laziness and avoiding intermediate `Vec`s

Iterator adaptors do nothing until consumed. That means you can chain freely without allocating between steps — so don't `collect` in the middle.

```rust
// ❌ two allocations just to count
let n = items.iter().filter(|i| i.active).collect::<Vec<_>>().len();

// ✅ zero allocations
let n = items.iter().filter(|i| i.active).count();
```

```rust
// ❌ collect then iterate again
let active: Vec<_> = items.iter().filter(|i| i.active).collect();
for item in active { process(item); }

// ✅
for item in items.iter().filter(|i| i.active) { process(item); }
```

Pitfall: because adaptors are lazy, `map` with side effects and no consumer does nothing (the compiler warns: `unused_must_use`). Use `for` or `for_each` for side effects.

Take `impl IntoIterator<Item = T>` in parameters when you only iterate once — callers can pass a `Vec`, an array, a slice iterator, or a chain without collecting:

```rust
fn total_len<'a>(words: impl IntoIterator<Item = &'a str>) -> usize {
    words.into_iter().map(str::len).sum()
}
```

## 6. Returning iterators

Return `impl Iterator<Item = T>` instead of building a `Vec` the caller might only scan once. The caller can still `collect()` if they need storage.

```rust
impl Inventory {
    // ✅ no allocation, lazily filtered
    pub fn in_stock(&self) -> impl Iterator<Item = &Item> + '_ {
        self.items.iter().filter(|i| i.quantity > 0)
    }
}
```

Use `Box<dyn Iterator<Item = T> + '_>` only when different branches return different iterator types and you can't unify them (or use `itertools::Either`/`either::Either`). For a custom iterator type with state, implement `Iterator` on a struct; implement `IntoIterator` for `&YourCollection` so `for x in &coll` works.

## 7. When a `for` loop is clearer

Iterators are not a religion. Prefer a plain `for` loop when:

- the body has side effects (I/O, logging, mutating several things),
- you need `break`/`continue` with complex conditions, or early `return` from the enclosing function with `?` in several places,
- the chain would need nested closures that capture mutably,
- a `fold` would carry a tuple of four accumulators.

```rust
// ✅ clearer as a loop: multiple fallible steps and side effects
for path in paths {
    let text = std::fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&text)?;
    registry.insert(cfg.name.clone(), cfg);
}
```

Rule of thumb: if the chain reads like a sentence, keep it; if you need comments to explain the chain, write the loop.

## 8. Choosing a collection

| Need | Use | Notes |
| --- | --- | --- |
| ordered list, push/pop at end, index | `Vec<T>` | default choice; contiguous, cache-friendly |
| queue / push & pop at both ends | `VecDeque<T>` | ring buffer |
| key → value, fast lookup, order irrelevant | `HashMap<K, V>` | default map |
| key → value, sorted keys, range queries | `BTreeMap<K, V>` | deterministic iteration order — good for output/snapshots |
| membership, unique items | `HashSet<T>` / `BTreeSet<T>` | same trade-off as maps |
| always pop the max (or min via `Reverse`) | `BinaryHeap<T>` | priority queues, Dijkstra, schedulers |
| map that remembers insertion order | `indexmap::IndexMap` | JSON-like objects, stable output |
| usually-small list, avoid heap | `smallvec::SmallVec<[T; N]>` / `arrayvec` | measure first |
| fixed size known at compile time | `[T; N]` | no heap at all |
| shared, immutable list | `Arc<[T]>` / `Box<[T]>` | smaller than `Vec`, no spare capacity |

`LinkedList` is almost never the right answer; `VecDeque` or `Vec` beat it in practice.

For many small `HashMap`s with trusted keys in hot paths, a faster hasher (`rustc-hash::FxHashMap`, `ahash`) helps; the default SipHash is DoS-resistant, which matters for keys from untrusted input.

## 9. `HashMap` idioms

### Entry API

**Why:** one lookup instead of two, and no "check then insert" race in logic.

```rust
// ❌ two lookups, and an unwrap
if !counts.contains_key(word) {
    counts.insert(word.to_string(), 0);
}
*counts.get_mut(word).unwrap() += 1;

// ✅
*counts.entry(word.to_string()).or_insert(0) += 1;

// grouping
groups.entry(user.team.clone()).or_default().push(user.id);

// expensive default only when missing
cache.entry(key).or_insert_with(|| compute(key));
```

`and_modify(..).or_insert(..)` handles "update if present, else insert". If allocating the key for `entry` is costly and hits are common, check with `get_mut(&str)` first, then fall back to `entry`.

### Look up with borrowed keys

`HashMap<String, V>` can be queried with `&str` because `String: Borrow<str>`. Don't allocate a `String` just to look up:

```rust
// ❌
let v = map.get(&name.to_string());
// ✅
let v = map.get(name); // name: &str
```

The same holds for `HashSet<String>`, `BTreeMap<Vec<u8>, _>` with `&[u8]`, `PathBuf`/`&Path`. For your own key types, implement `Borrow` only if `Hash`/`Eq`/`Ord` agree between the owned and borrowed forms.

Iterate with `for (k, v) in &map`, `map.keys()`, `map.values()`, `map.values_mut()`. Remove conditionally with `map.retain(|k, v| ...)`.

## 10. `Vec` idioms

```rust
// Preallocate when the size is known — avoids repeated reallocation
let mut out = Vec::with_capacity(input.len());

// Remove in place without allocating a new Vec
users.retain(|u| u.active);

// Move a range out (e.g. process a batch, keep the rest)
let batch: Vec<Job> = queue.drain(..batch_size.min(queue.len())).collect();

// Remove consecutive duplicates — sort first for global uniqueness
tags.sort_unstable();
tags.dedup();

// Sort by a derived key; sort_unstable is faster when equal elements' order doesn't matter
users.sort_by_key(|u| u.age);
users.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(b.age.cmp(&a.age)));
// Key is expensive (allocates)? compute it once per element
files.sort_by_cached_key(|f| f.name.to_lowercase());

// Search a *sorted* Vec in O(log n)
match sorted_ids.binary_search(&id) {
    Ok(idx) => println!("found at {idx}"),
    Err(insert_at) => sorted_ids.insert(insert_at, id), // keeps it sorted
}

// Remove by index in O(1) when order doesn't matter
let removed = v.swap_remove(i);

// Split into owned parts, extend from iterators
v.extend(other.iter().copied());
let tail = v.split_off(mid);
```

Use slice methods freely on `Vec` — `first`, `last`, `get(i)` (returns `Option`, no panic), `split_first`, `contains`, `iter().rev()`, `chunks`, `concat`, `join`, `to_vec`.

Slice patterns are often cleaner than index arithmetic:

```rust
match args.as_slice() {
    [] => usage(),
    [cmd] => run(cmd, &[]),
    [cmd, rest @ ..] => run(cmd, rest),
}
```

## 11. Pitfalls

- **Indexing panics.** `v[i]` and `map[&k]` panic when missing; use `get` unless the index is proven valid.
- **`collect` without a type.** The compiler needs to know the target; annotate the binding.
- **Holding an iterator borrow while mutating** the same collection is a borrow error — collect the keys/indices you need first, or use `retain`/`drain`/`entry`.
- **`chars().count()` is O(n)** and counts Unicode scalar values, not bytes (`len()`) or graphemes (`unicode-segmentation`).
- **`sort` on floats** doesn't compile because `f64: !Ord`; use `sort_by(|a, b| a.total_cmp(b))`.
- **Iteration order of `HashMap` is random** — use `BTreeMap`/`IndexMap` or sort before printing, snapshotting or hashing output.
- **`dedup` only removes adjacent duplicates.**
- **`binary_search` on unsorted data** gives meaningless results, silently.
- **Long chains with `.clone()` inside** usually mean you should iterate by reference or `into_iter` the owner.
