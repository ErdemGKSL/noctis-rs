# Traits, `impl` Blocks, and Generics

Use traits to make code flexible and reusable without inheritance, and choose the
right kind of polymorphism for each job.

**Contents**
1. Small, focused traits
2. Organizing `impl` blocks
3. Generics vs `impl Trait` vs `dyn Trait`
4. Associated types vs generic parameters
5. Default methods
6. Blanket impls
7. Extension traits
8. Standard traits to implement (and conversion traits)
9. `Deref`: when it's right, and the anti-pattern
10. Sealed traits
11. Trait objects and dyn compatibility
12. `where` clauses and bounds hygiene
13. The orphan rule and the newtype workaround
14. Operator overloading
15. `Iterator` / `IntoIterator` for your own types

---

## 1. Small, focused traits

**Why:** a trait is a contract. The smaller the contract, the more types can satisfy it
and the easier it is to mock in tests. Large "god traits" force implementors to stub out
methods they don't support.

```rust
// ❌ one trait, many unrelated responsibilities
trait Storage {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn put(&mut self, key: &str, val: Vec<u8>);
    fn backup_to_s3(&self);
    fn metrics(&self) -> String;
}
```

```rust
// ✅ split by capability; compose with bounds
pub trait Read { fn get(&self, key: &str) -> Option<Vec<u8>>; }
pub trait Write { fn put(&mut self, key: &str, val: Vec<u8>); }

pub fn copy_key<S: Read, D: Write>(src: &S, dst: &mut D, key: &str) {
    if let Some(v) = src.get(key) { dst.put(key, v); }
}
```

Define a trait when you have (or concretely expect) **more than one implementation**,
or need a seam for testing. A trait with a single implementor "for flexibility" is
indirection without benefit. Start with an inherent `impl` and extract a trait when the
second implementation shows up.

## 2. Organizing `impl` blocks

- Put constructors (`new`, `with_capacity`, `from_parts`) first in the inherent
  `impl`, then accessors, then behaviour.
- Use one `impl Trait for Type` block per trait, placed after the inherent block.
- Splitting the inherent impl into several blocks is fine when it groups related
  methods, or when some methods need extra bounds:

```rust
pub struct Wrapper<T>(Vec<T>);

impl<T> Wrapper<T> {
    pub fn new() -> Self { Self(Vec::new()) }
    pub fn push(&mut self, v: T) { self.0.push(v); }
}

// Only available when T can be ordered. No bound needed on the whole type.
impl<T: Ord> Wrapper<T> {
    pub fn max(&self) -> Option<&T> { self.0.iter().max() }
}
```

**Put bounds on `impl` blocks and functions, not on the struct definition.** A bound on
`struct Wrapper<T: Ord>` must then be repeated on every impl and every user, even where
ordering is irrelevant.

## 3. Generics vs `impl Trait` vs `dyn Trait`

| Tool | Dispatch | Use when | Cost |
| --- | --- | --- | --- |
| `fn f<T: Trait>(x: T)` | static (monomorphized) | caller picks the type; you need to name `T` twice (`fn eq<T: Eq>(a: T, b: T)`), or callers may want turbofish | code size grows per instantiation |
| `fn f(x: impl Trait)` | static | argument position shorthand for a single, unnamed generic | same as generics; no turbofish |
| `fn f() -> impl Trait` | static | return an unnameable/private type (closures, iterator chains) | one concrete type per function |
| `&dyn Trait` / `Box<dyn Trait>` | dynamic (vtable) | heterogeneous collections, plugin registries, reducing compile time/binary size, runtime-chosen implementations | indirect call, no inlining, heap alloc for `Box` |

```rust
use std::fmt::Display;

// Static dispatch, fast, inlinable:
pub fn print_all(items: &[impl Display]) {
    for i in items { println!("{i}"); }
}

// Return position: hides the concrete iterator type.
pub fn evens(limit: u32) -> impl Iterator<Item = u32> {
    (0..limit).filter(|n| n % 2 == 0)
}

// Dynamic dispatch: one Vec, many types.
pub trait Shape { fn area(&self) -> f64; }
pub struct Circle(pub f64);
pub struct Square(pub f64);
impl Shape for Circle { fn area(&self) -> f64 { std::f64::consts::PI * self.0 * self.0 } }
impl Shape for Square { fn area(&self) -> f64 { self.0 * self.0 } }

pub fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}
```

Default to generics/`impl Trait`; switch to `dyn` when you genuinely need runtime
heterogeneity. If the set of types is closed and known, an **enum** is often better than
both: static dispatch, no allocation, exhaustive matching.

**Accept flexible inputs:** `impl AsRef<str>`, `impl AsRef<Path>`, `impl Into<String>`,
`impl IntoIterator<Item = T>` let callers pass whatever they have without converting.
Keep the generic shim thin to limit code bloat:

```rust
use std::path::Path;

pub fn load(path: impl AsRef<Path>) -> std::io::Result<String> {
    fn inner(path: &Path) -> std::io::Result<String> { std::fs::read_to_string(path) }
    inner(path.as_ref()) // the heavy body is compiled once
}
```

## 4. Associated types vs generic parameters

- **Associated type** (`type Item;`): there is exactly **one** natural choice per
  implementing type. `Iterator::Item`, `Deref::Target`, `FromStr::Err`. Callers don't
  have to annotate it, and bounds read better (`I: Iterator<Item = u8>`).
- **Generic parameter** (`trait From<T>`): a type can implement the trait **many
  times** for different `T`.

```rust
pub trait Parser {
    type Output;                       // one output type per parser
    fn parse(&self, input: &str) -> Option<Self::Output>;
}

pub trait Convert<T> {                 // a type can convert into many targets
    fn convert(&self) -> T;
}
```

## 5. Default methods

Provide a minimal set of required methods and build the rest as default methods on top.
Implementors get a rich API for free and can still override for efficiency.

```rust
pub trait Repository {
    fn find(&self, id: u64) -> Option<String>;

    fn exists(&self, id: u64) -> bool { self.find(id).is_some() }
    fn find_many(&self, ids: &[u64]) -> Vec<String> {
        ids.iter().filter_map(|&id| self.find(id)).collect()
    }
}
```

This is how `Iterator` works: implement `next`, get 70+ adapters.

## 6. Blanket impls

Implement a trait for every type that satisfies a bound. One impl covers types you
haven't even written yet.

```rust
use std::fmt::Display;

pub trait Describe { fn describe(&self) -> String; }

impl<T: Display> Describe for T {
    fn describe(&self) -> String { format!("<{self}>") }
}
```

Also common: forward a trait through references and smart pointers so callers can pass
`&T`, `Box<T>`, or `Arc<T>`:

```rust
pub trait Notifier { fn notify(&self, msg: &str); }

impl<T: Notifier + ?Sized> Notifier for &T { fn notify(&self, m: &str) { (**self).notify(m) } }
impl<T: Notifier + ?Sized> Notifier for Box<T> { fn notify(&self, m: &str) { (**self).notify(m) } }
impl<T: Notifier + ?Sized> Notifier for std::sync::Arc<T> { fn notify(&self, m: &str) { (**self).notify(m) } }
```

**Pitfall:** a blanket `impl<T: Display> MyTrait for T` in a library forbids you (and
downstream crates) from writing any more specific impl that could overlap, because Rust
has no specialization. Adding a blanket impl to a published trait is a breaking change.

## 7. Extension traits

Add methods to types you don't own (std, other crates) by defining a trait and
implementing it for them. Convention: name it `FooExt`.

```rust
pub trait StrExt {
    fn truncate_ellipsis(&self, max: usize) -> String;
}

impl StrExt for str {
    fn truncate_ellipsis(&self, max: usize) -> String {
        match self.char_indices().nth(max) {
            Some((idx, _)) => format!("{}…", &self[..idx]),
            None => self.to_owned(),
        }
    }
}

// use crate::StrExt;  "hello world".truncate_ellipsis(5) == "hello…"
```

Callers must import the trait for the methods to resolve, so keep extension traits in a
`prelude` module or re-export them where they're needed. Use them sparingly: a free
function is simpler when only one call site needs the helper.

## 8. Standard traits to implement

Implementing std traits lets your types plug into the whole ecosystem: `?`, `.parse()`,
`format!`, `collect()`, `HashMap` keys, sorting, and so on.

| Trait | Implement when | Note |
| --- | --- | --- |
| `Debug` | always, on every public type | derive; redact secrets manually |
| `Clone`, `Copy` | value semantics; `Copy` only for small, plain data | adding `Copy` later is fine; removing it breaks users |
| `PartialEq`, `Eq`, `Hash` | equality/keys | `Hash` must agree with `Eq` |
| `PartialOrd`, `Ord` | natural ordering exists | derive orders by field/variant order |
| `Default` | an obvious "empty" value exists | enables `..Default::default()` and `#[serde(default)]` |
| `Display` | user-facing text | not for debugging; gives `.to_string()` for free |
| `From<T>` | lossless, infallible conversion | gives `Into` for free; powers `?` error conversion |
| `TryFrom<T>` | fallible conversion / validation | gives `TryInto` |
| `FromStr` | parse from text | enables `"..".parse::<T>()`, clap, etc. |
| `AsRef<T>` | cheap reference conversion | `impl AsRef<str> for Email` |
| `Borrow<T>` | your owned type should look up `HashMap<Owned, _>` by `&T` | `Hash`/`Eq`/`Ord` must match between the two |
| `Error` | error types | see `error-handling.md` |

```rust
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Celsius(pub i32);

impl fmt::Display for Celsius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}°C", self.0) }
}

impl From<i32> for Celsius {
    fn from(v: i32) -> Self { Celsius(v) }
}
```

Rules of thumb:
- **Implement `From`, never `Into`.** `Into` comes from a blanket impl.
- **`From` must not fail or lose information.** If it can, use `TryFrom`.
- **Don't implement `Display` just to get `to_string()`** on internal types. Only
  implement it when there's one obvious human-readable form.
- **Prefer `#[derive]`** over manual impls; write manual ones only when semantics
  differ (e.g. `PartialEq` that ignores a cache field, `Debug` that hides a password).

## 9. `Deref`: when it's right, and the anti-pattern

`Deref` is for **smart pointers** (`Box`, `Arc`, `String → str`, `Vec<T> → [T]`):
types whose whole purpose is to point at a `Target`.

**Anti-pattern:** using `Deref` to emulate inheritance, so `Child` "inherits" `Parent`'s
methods:

```rust
// ❌ confusing: method resolution silently jumps to another type,
//    and trait impls on Parent do NOT carry over to Admin
struct Parent { name: String }
struct Admin { inner: Parent }
impl std::ops::Deref for Admin { type Target = Parent; fn deref(&self) -> &Parent { &self.inner } }
```

Prefer explicit delegation methods, a shared trait, or composition with a public
accessor (`admin.user().name`). For validated newtypes, `Deref<Target = str>` is
acceptable when the newtype really is "a string with a guarantee", but never
`DerefMut`, which would let callers break the guarantee.

## 10. Sealed traits

A public trait that downstream crates may *use* but not *implement*, so you can add
methods without a breaking change:

```rust
mod private { pub trait Sealed {} }

pub trait Backend: private::Sealed {
    fn name(&self) -> &'static str;
}

pub struct Postgres;
impl private::Sealed for Postgres {}
impl Backend for Postgres { fn name(&self) -> &'static str { "postgres" } }
```

`private` isn't nameable outside the crate, so nobody else can satisfy the supertrait.
Document that the trait is sealed.

## 11. Trait objects and dyn compatibility

A trait can be used as `dyn Trait` only if it's *dyn compatible* (formerly "object
safe"). The usual blockers:

- methods with generic type parameters (`fn f<T>(&self, t: T)`),
- methods returning `Self` or taking `Self` by value,
- associated consts, or a `Self: Sized` supertrait.

Opt individual methods out with `where Self: Sized` so the rest stays usable through
`dyn`:

```rust
pub trait Plugin {
    fn name(&self) -> &str;
    fn run(&self, input: &str) -> String;

    fn boxed(self) -> Box<dyn Plugin> where Self: Sized + 'static { Box::new(self) }
}

pub struct Upper;
impl Plugin for Upper {
    fn name(&self) -> &str { "upper" }
    fn run(&self, input: &str) -> String { input.to_uppercase() }
}

pub fn registry() -> Vec<Box<dyn Plugin>> { vec![Upper.boxed()] }
```

More notes:
- For shared ownership across threads use `Arc<dyn Trait + Send + Sync>`; spell out the
  auto traits you need.
- Since Rust 1.75, `async fn` works in traits for static dispatch. For `dyn` with async
  methods, use the `async-trait` crate, or return
  `Pin<Box<dyn Future<Output = T> + Send + '_>>` manually.
- Since Rust 1.86 you can upcast `&dyn Sub` to `&dyn Super` directly.

## 12. `where` clauses and bounds hygiene

Move bounds to a `where` clause when there's more than one or they're long. Signatures
stay readable:

```rust
use std::{collections::HashMap, fmt::Debug, hash::Hash};

pub fn group_by<I, K, V, F>(items: I, key: F) -> HashMap<K, Vec<V>>
where
    I: IntoIterator<Item = V>,
    K: Eq + Hash,
    F: Fn(&V) -> K,
    V: Debug,
{
    let mut map: HashMap<K, Vec<V>> = HashMap::new();
    for item in items {
        map.entry(key(&item)).or_default().push(item);
    }
    map
}
```

- Require only what the body uses. Every extra bound (`Clone`, `Debug`, `Send`) narrows
  who can call you. (The `V: Debug` above is an example of a bound to remove.)
- Prefer `Fn` < `FnMut` < `FnOnce` in what you *require*: accepting `FnOnce` is the most
  permissive for callers if you call it once; accept `FnMut` if you call it repeatedly.
- Name a bound set once with a trait alias pattern if it repeats a lot:
  `pub trait Key: Eq + Hash + Clone + Send + Sync + 'static {}` plus a blanket
  `impl<T: Eq + Hash + Clone + Send + Sync + 'static> Key for T {}`.

## 13. The orphan rule and the newtype workaround

You may implement a trait for a type only if the trait **or** the type is defined in
your crate. To implement a foreign trait for a foreign type, wrap the type:

```rust
use std::fmt;

pub struct CommaList(pub Vec<String>);   // our type, wraps a foreign one

impl fmt::Display for CommaList {         // foreign trait, now allowed
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join(", "))
    }
}
```

Expose `into_inner()`/`AsRef` for access, and resist adding `Deref` just to forward
everything (see section 9).

## 14. Operator overloading

Implement `std::ops` traits only when the operator's meaning is unsurprising: math on
vectors, money, durations, set union. Don't make `+` mean "append to log".

```rust
use std::ops::{Add, Mul};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 { pub x: f64, pub y: f64 }

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 { Vec2 { x: self.x + o.x, y: self.y + o.y } }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 { Vec2 { x: self.x * k, y: self.y * k } }
}

impl std::iter::Sum for Vec2 {
    fn sum<I: Iterator<Item = Vec2>>(iter: I) -> Vec2 { iter.fold(Vec2::default(), Add::add) }
}
```

Implement the matching `*Assign` traits (`AddAssign`) too, and `Sum`/`Product` so
iterators can `.sum()` your type. For money, define `Add` only between the same
currency, or return `Option`/`Result` through a method instead of an operator.

## 15. `Iterator` / `IntoIterator` for your own types

If your type is a collection, implement `IntoIterator` for `T`, `&T` and `&mut T` so it
works in `for` loops and with every iterator adapter.

```rust
pub struct Playlist { songs: Vec<String> }

impl Playlist {
    pub fn iter(&self) -> std::slice::Iter<'_, String> { self.songs.iter() }
}

impl IntoIterator for Playlist {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;
    fn into_iter(self) -> Self::IntoIter { self.songs.into_iter() }
}

impl<'a> IntoIterator for &'a Playlist {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter { self.songs.iter() }
}

impl FromIterator<String> for Playlist {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Playlist { songs: iter.into_iter().collect() }
    }
}

impl Extend<String> for Playlist {
    fn extend<I: IntoIterator<Item = String>>(&mut self, iter: I) { self.songs.extend(iter) }
}
```

For a custom iteration order, write a struct that holds the iteration state and
implement `Iterator::next`. Also implement `size_hint` (and `ExactSizeIterator`) when
the length is known, so `collect` can pre-allocate. For simple cases,
`std::iter::from_fn` or returning `impl Iterator` saves you the struct.
