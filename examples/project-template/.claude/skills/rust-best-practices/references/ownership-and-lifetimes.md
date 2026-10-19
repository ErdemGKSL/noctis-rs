# Ownership, Borrowing and Lifetimes

How to choose between owning, borrowing and sharing data so code is fast, flexible and doesn't fight the borrow checker.

## Contents

1. [Parameters: borrow the most general form](#1-parameters-borrow-the-most-general-form)
2. [Return values: return owned data](#2-return-values-return-owned-data)
3. [`Cow`: maybe-owned data](#3-cow-maybe-owned-data)
4. [Clone discipline and `Copy`](#4-clone-discipline-and-copy)
5. [Closures and moves](#5-closures-and-moves)
6. [Lifetimes: elision and when to annotate](#6-lifetimes-elision-and-when-to-annotate)
7. [Structs: own or borrow?](#7-structs-own-or-borrow)
8. [Splitting borrows and `mem::take` / `replace` / `swap`](#8-splitting-borrows-and-memtake--replace--swap)
9. [Shared ownership and interior mutability](#9-shared-ownership-and-interior-mutability)
10. [Graphs, cycles and self-references: use indices](#10-graphs-cycles-and-self-references-use-indices)
11. [Common borrow-checker errors and fixes](#11-common-borrow-checker-errors-and-fixes)

---

## 1. Parameters: borrow the most general form

Take the weakest form of access the function needs. Callers can always lend more than you
ask for; they can't lend less.

| Need | Parameter type | Accepts |
| --- | --- | --- |
| read text | `&str` | `&String`, `&str`, literals, `&Box<str>` |
| read a sequence | `&[T]` | `&Vec<T>`, arrays, slices |
| read a path | `impl AsRef<Path>` / `&Path` | `&str`, `String`, `PathBuf`, `&Path` |
| mutate in place | `&mut T` / `&mut [T]` | |
| store it (needs ownership) | `T` / `impl Into<String>` | moves in; caller decides whether to clone |
| iterate once | `impl IntoIterator<Item = T>` | vecs, arrays, iterators, sets |

```rust
// ❌ forces callers to own a String and a Vec
fn count_matches(needle: &String, haystack: &Vec<String>) -> usize { todo!() }

// ✅ works with anything string-like / slice-like
fn count_matches(needle: &str, haystack: &[String]) -> usize {
    haystack.iter().filter(|s| s.contains(needle)).count()
}
```

**Take ownership when you store the value.** If the function will keep the data, accept
`String` (or `impl Into<String>`) instead of `&str` and cloning inside — the caller may
already have a `String` it no longer needs, and moving it is free:

```rust
pub struct User { name: String }

impl User {
    // caller with a String: moved, zero copies; caller with &str: one allocation
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}
```

Avoid `impl AsRef`/`impl Into` generics on large internal functions that are called with
one type — each instantiation is compiled separately. A common trick is a thin generic
shell over a non-generic inner function:

```rust
pub fn load(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    fn inner(path: &std::path::Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    inner(path.as_ref())
}
```

## 2. Return values: return owned data

Return `String`, `Vec<T>`, `PathBuf` rather than references into temporaries — you can't
return a reference to something created inside the function anyway. Return borrowed data
only when it borrows **from an input** (`fn first_word(s: &str) -> &str`), which elision
handles for you.

Returning an iterator instead of a `Vec` avoids allocation when the caller just loops:

```rust
pub fn active_names(users: &[User]) -> impl Iterator<Item = &str> + '_ {
    users.iter().filter(|u| u.active).map(|u| u.name.as_str())
}
pub struct User { name: String, active: bool }
```

## 3. `Cow`: maybe-owned data

`Cow<'a, str>` holds either a borrowed `&'a str` or an owned `String`. Use it when a
function *usually* returns its input unchanged and only *sometimes* needs to allocate.

```rust
use std::borrow::Cow;

/// Escapes `<` and `>`; allocates only if the input contains them.
pub fn escape_html(input: &str) -> Cow<'_, str> {
    if !input.contains(['<', '>']) {
        return Cow::Borrowed(input);
    }
    Cow::Owned(input.replace('<', "&lt;").replace('>', "&gt;"))
}
```

Also useful in serde structs (`#[serde(borrow)] name: Cow<'a, str>`) to borrow from the
input buffer when no unescaping is needed. Don't reach for `Cow` by default — it adds a
lifetime and a branch; use it where profiling or the API shape justifies it.

## 4. Clone discipline and `Copy`

`clone()` is not evil — it's often the simplest correct choice for small data or cold
paths. The smell is cloning **to silence the borrow checker**, especially inside loops.

```rust
// ❌ clones every item just to read it
for user in users.clone() {
    println!("{}", user.name);
}

// ✅ borrow
for user in &users {
    println!("{}", user.name);
}
```

Before adding `.clone()`, ask:

1. Can I borrow instead (`&`, `.iter()`, `.as_str()`, `.as_deref()`)?
2. Can I move instead (is this the last use)?
3. Is the data shared for a long time? Then `Arc<T>`/`Rc<T>` — cloning a pointer, not the data.
4. Is it a large immutable string/slice shared widely? `Arc<str>` / `Arc<[T]>` are cheaper
   to clone than `String`/`Vec` and smaller than `Arc<String>`.

**Derive `Copy` for small, plain values** (ids, coordinates, enums without data, flags):
`#[derive(Clone, Copy)] struct Point { x: f32, y: f32 }`. Rough guide: ≤ 16–24 bytes, no
heap ownership, semantically a value. Don't derive `Copy` on types that may grow heap
fields later — removing `Copy` is a breaking change.

Make clones visible in hot paths: `Arc::clone(&cfg)` reads as "cheap pointer copy",
`cfg.clone()` doesn't.

## 5. Closures and moves

- Closures borrow what they capture by default; `move` makes them take ownership.
  `std::thread::spawn` and `tokio::spawn` need `'static` closures, hence `move`.
- Since edition 2021 closures capture **disjoint fields**, so `|| self.a.len()` borrows only
  `self.a`.
- To move a clone into a closure while keeping the original, clone into a new binding
  right before it:

```rust
use std::sync::Arc;

fn spawn_workers(config: Arc<Config>) {
    for id in 0..4 {
        let config = Arc::clone(&config); // shadow: each thread gets its own handle
        std::thread::spawn(move || run(id, &config));
    }
}
pub struct Config;
fn run(_id: u32, _c: &Config) {}
```

- Prefer `std::thread::scope` when threads only need to borrow local data — no `Arc`, no
  `'static`, no `move` of clones:

```rust
fn parallel_sum(chunks: &[Vec<u64>]) -> u64 {
    std::thread::scope(|s| {
        let handles: Vec<_> = chunks.iter().map(|c| s.spawn(|| c.iter().sum::<u64>())).collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).sum()
    })
}
```

## 6. Lifetimes: elision and when to annotate

Most functions need no lifetime annotations thanks to elision:

1. Each elided input reference gets its own lifetime.
2. If there's exactly one input lifetime, outputs get it.
3. If there's `&self`/`&mut self`, outputs get `self`'s lifetime.

Annotate when the output borrows from a **specific** one of several inputs:

```rust
// The result borrows from `haystack`, not from `needle`.
fn find_line<'h>(haystack: &'h str, needle: &str) -> Option<&'h str> {
    haystack.lines().find(|l| l.contains(needle))
}
```

Tips:

- Use `'_` to make an elided lifetime visible in types: `fn iter(&self) -> Iter<'_, T>`,
  `impl fmt::Display for Wrapper<'_>`.
- `'static` in a bound (`T: 'static`) means "contains no non-static borrows", **not** "lives
  forever" — `String` is `'static`.
- `&'static str` is right for string literals and leaked/const data; don't demand it for
  arbitrary input — take `&str` or `String`.
- If lifetime annotations spread through half the codebase, it's usually a sign a struct
  should own its data (next section).

## 7. Structs: own or borrow?

**Default to owning.** A struct with owned fields (`String`, `Vec`, `Arc`) can be stored,
returned, sent across threads and put into collections freely.

Borrowing structs (`struct Parser<'a> { input: &'a str }`) are great for **short-lived
views**: parsers, iterators, zero-copy deserialization, temporary "context" bundles of
references passed to helpers. They should not be stored long-term or put in app state.

```rust
// ✅ short-lived view over borrowed input
pub struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        let s = self.rest.trim_start();
        if s.is_empty() {
            return None;
        }
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        let (tok, rest) = s.split_at(end);
        self.rest = rest;
        Some(tok)
    }
}
```

Note `Option<&'a str>` (lifetime of the input), not `Option<&str>` tied to `&mut self` —
that's what lets callers keep tokens after the iterator moves on.

## 8. Splitting borrows and `mem::take` / `replace` / `swap`

The borrow checker reasons about **fields**, not methods. Borrowing two fields directly
works; calling `&mut self` methods while holding a field borrow does not.

```rust
struct Game { players: Vec<Player>, log: Vec<String> }
struct Player { name: String, score: u32 }

impl Game {
    // ❌ self.log(..) borrows all of self while `p` borrows self.players
    // fn award(&mut self) { for p in &mut self.players { p.score += 1; self.log(&p.name); } }

    // ✅ borrow the fields separately
    fn award(&mut self) {
        let Self { players, log } = self;
        for p in players.iter_mut() {
            p.score += 1;
            log.push(format!("{} scored", p.name));
        }
    }
}
```

Other splitting tools: `slice.split_at_mut(i)`, `slice.split_first_mut()`,
`iter_mut()`, `HashMap::get_disjoint_mut` (1.86+) for two entries at once.

**Moving out of `&mut`**: you can't leave a hole, but you can swap something in.

```rust
use std::mem;

enum State { Idle, Running { buffer: Vec<u8> }, Done(Vec<u8>) }

fn finish(state: &mut State) {
    // take the buffer out, leaving an empty Vec behind (no clone)
    if let State::Running { buffer } = state {
        let data = mem::take(buffer);
        *state = State::Done(data);
    }
}

fn reset(state: &mut State) -> State {
    mem::replace(state, State::Idle) // returns the old value
}
```

- `mem::take(&mut x)` — replace with `Default::default()`, return the old value.
- `mem::replace(&mut x, new)` — replace with a given value.
- `mem::swap(&mut a, &mut b)` — exchange two values.
- `Option::take()` — `mem::take` for options, the classic way to move out of a field:
  `if let Some(handle) = self.worker.take() { handle.join()… }`.
- `Option::replace(v)`, `Option::get_or_insert_with(f)` round out the set.

## 9. Shared ownership and interior mutability

Reach for these when one owner genuinely isn't the right model — not as a reflex.

| Need | Single thread | Multi-thread |
| --- | --- | --- |
| shared, immutable | `Rc<T>` | `Arc<T>` |
| shared, mutable | `Rc<RefCell<T>>` | `Arc<Mutex<T>>` / `Arc<RwLock<T>>` |
| mutate a `Copy` value through `&` | `Cell<T>` | atomics (`AtomicU64`, `AtomicBool`) |
| initialize once, read many | `OnceCell<T>` | `OnceLock<T>` / `LazyLock<T>` |
| break `Rc`/`Arc` cycles (back-pointers) | `Weak<T>` | `Weak<T>` |

Guidelines:

- **Prefer ownership + `&mut` first.** Interior mutability moves borrow checking to runtime
  (`RefCell` panics on double `borrow_mut`; `Mutex` can deadlock).
- **Keep critical sections short**: lock, copy/modify, drop the guard. Never hold a
  `std::sync::Mutex` guard across `.await` (see `async-and-concurrency.md`).
- **`RwLock`** helps only with many readers and rare writers; under contention a `Mutex` is
  often as fast and simpler.
- For counters/flags, atomics beat `Mutex<u64>`.
- `Arc<T>` gives shared *immutable* access; if `T` is read-only config, you don't need a
  lock at all. To update rarely, swap the whole `Arc` (the `arc-swap` crate does this well).
- Globals: `static CACHE: LazyLock<Mutex<HashMap<..>>> = LazyLock::new(Default::default);` —
  but prefer passing state explicitly; globals hurt testability.

```rust
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Stats { hits: u64 }

fn record(stats: &Arc<Mutex<Stats>>) {
    // `lock` returns Err only if another thread panicked while holding it
    let mut guard = stats.lock().expect("stats mutex poisoned");
    guard.hits += 1;
} // guard dropped here — lock released
```

## 10. Graphs, cycles and self-references: use indices

A struct that holds both data and references into that data (self-referential) can't be
expressed with plain references — moving the struct would invalidate them. Trees with
parent pointers and graphs hit the same wall.

**Prefer indices (or keyed ids) into a collection** over `Rc<RefCell<…>>` webs:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeId(usize);

pub struct Tree {
    nodes: Vec<Node>,
}

pub struct Node {
    pub value: String,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

impl Tree {
    pub fn add(&mut self, value: impl Into<String>, parent: Option<NodeId>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node { value: value.into(), parent, children: Vec::new() });
        if let Some(p) = parent {
            self.nodes[p.0].children.push(id);
        }
        id
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }
}
```

**Why:** indices are `Copy`, need no lifetimes, serialize trivially and are cache friendly.
Wrap them in newtypes so ids from different collections can't be mixed. If nodes get
removed, use a generational arena (`slotmap`, `generational-arena`) so stale ids are
detected instead of silently pointing at a new node. Alternatively store the owned data
and keep **offsets/ranges** instead of `&str` slices into it.

Self-referential crates (`ouroboros`, `self_cell`) exist for the rare cases where none of
this fits; reach for them last.

## 11. Common borrow-checker errors and fixes

| Error | Typical cause | Fix |
| --- | --- | --- |
| E0502 "cannot borrow as mutable because it is also borrowed as immutable" | holding a reference (e.g. from `.get()`/`.iter()`) while mutating the collection | finish using the reference first; copy out the needed value (`.copied()`, clone a small field); collect indices/keys then mutate; use the entry API |
| E0499 "cannot borrow as mutable more than once" | two `&mut` into the same collection/struct | split borrows (§8), `split_at_mut`, `get_disjoint_mut`, restructure into two steps |
| E0505/E0382 "use of moved value" | value moved into a function/closure/loop and used later | borrow instead, clone before the move, or move after last use; iterate `&v` not `v` |
| E0507 "cannot move out of borrowed content" / "out of index" | `let x = vec[i];` or `self.field` by value through `&self` | borrow (`&vec[i]`), `.clone()`, `mem::take`, `Option::take`, or `swap_remove`/`remove` |
| E0515 "cannot return reference to local variable" | returning `&` to data created in the function | return the owned value |
| E0597 "does not live long enough" | reference outlives its owner (often stored in a struct or sent to a thread) | own the data, extend the owner's scope, use `Arc`, or `thread::scope` |
| E0716 "temporary value dropped while borrowed" | `let s = String::from("x").as_str();` | bind the owner to a variable first |
| "borrowed data escapes outside of closure"/`'static` required | `thread::spawn`/`tokio::spawn` with borrowed captures | `move` owned/`Arc` clones in, or use scoped threads |

The classic "check then mutate a map" case is solved by the entry API rather than a
double lookup:

```rust
use std::collections::HashMap;

fn count_words(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}
```

When the borrow checker rejects something that "should" work, first try restructuring the
data flow (shorter borrows, two passes, owning instead of borrowing). Only then reach for
`Rc`/`RefCell`, and almost never for `unsafe`.
