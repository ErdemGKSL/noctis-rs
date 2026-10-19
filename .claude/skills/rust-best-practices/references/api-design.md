# API Design

How to shape the public surface of a crate or module so it is easy to use correctly, hard to misuse, and safe to evolve. Based on the Rust API Guidelines (rust-lang.github.io/api-guidelines).

## Contents

1. [Naming conventions](#1-naming-conventions)
2. [Constructors and `Default`](#2-constructors-and-default)
3. [Builders](#3-builders)
4. [Accepting arguments: generic or concrete?](#4-accepting-arguments)
5. [Visibility: minimal surface](#5-visibility-minimal-surface)
6. [Module layout and re-exports](#6-module-layout-and-re-exports)
7. [Implement the common traits](#7-implement-the-common-traits)
8. [`#[must_use]` and `#[non_exhaustive]`](#8-must_use-and-non_exhaustive)
9. [Documentation](#9-documentation)
10. [Feature flags](#10-feature-flags)
11. [Semver hazards](#11-semver-hazards)
12. [Pitfalls](#12-pitfalls)

---

## 1. Naming conventions

**Why:** Rust users read method names as contracts about cost and ownership. Following the conventions lets them predict behavior without reading docs.

Casing: `UpperCamelCase` for types/traits/enum variants, `snake_case` for functions/methods/modules/variables, `SCREAMING_SNAKE_CASE` for consts/statics. Acronyms are words: `HttpClient`, `Uuid`, not `HTTPClient`.

### Conversion prefixes

| Prefix | Cost | Ownership | Example |
| --- | --- | --- | --- |
| `as_` | free | `&self` → borrowed view | `str::as_bytes`, `String::as_str` |
| `to_` | expensive (allocates/computes) | `&self` → owned | `str::to_lowercase`, `Path::to_path_buf` |
| `into_` | usually cheap | `self` (consumed) → owned | `String::into_bytes`, `Vec::into_boxed_slice` |

```rust
impl Email {
    pub fn as_str(&self) -> &str { &self.0 }          // free view
    pub fn into_inner(self) -> String { self.0 }      // consumes, no copy
    pub fn to_lowercase(&self) -> Email { /* allocates */ Email(self.0.to_lowercase()) }
}
```

### Getters, iterators, constructors

- **Getters have no `get_` prefix:** `user.name()`, not `user.get_name()`. Mutable access is `name_mut()`. (`get` alone is fine for indexed/keyed lookup: `map.get(k)`.)
- **Iterator methods:** `iter()` → `&T`, `iter_mut()` → `&mut T`, `into_iter()` → `T`. Iterator *types* are named after the method: `Iter`, `IterMut`, `IntoIter`.
- **Constructors:** `new` for the primary one; `with_capacity`/`with_*` for variants taking a configuration; `from_*` for conversions from another kind (`from_str_radix`, `from_utf8`); `try_*` for fallible versions (`try_new`, `try_from`).
- **Predicates:** `is_*` / `has_*` / `can_*` return `bool`.
- Avoid stuttering: in module `config`, call the type `Config`, not `ConfigConfig`; users write `config::Config` or import it.

## 2. Constructors and `Default`

Provide `new()` as the obvious entry point. If `new()` takes no arguments, also implement `Default` (clippy's `new_without_default` will remind you) so the type works with `..Default::default()`, `unwrap_or_default`, `#[serde(default)]`, and `mem::take`.

```rust
#[derive(Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
}

impl Registry {
    pub fn new() -> Self { Self::default() }
}
```

For validated types, the constructor is fallible and the fields are private, so an invalid value can never exist:

```rust
pub struct Port(u16);

impl Port {
    pub fn new(n: u16) -> Result<Self, PortError> {
        if n == 0 { return Err(PortError::Zero); }
        Ok(Self(n))
    }
    pub fn get(self) -> u16 { self.0 }
}
```

Prefer implementing `TryFrom<u16>`/`FromStr` too, so the type plugs into `?`, `.parse()` and serde's `try_from`.

## 3. Builders

**Why:** more than ~3 optional parameters make positional constructors unreadable (`Client::new(None, Some(30), true, None)`), and adding a parameter is a breaking change. A builder names every option and can grow without breaking callers.

```rust
#[derive(Debug, Clone)]
pub struct Client { base_url: String, timeout: Duration, retries: u32, user_agent: String }

#[derive(Debug, Clone)]
#[must_use = "a builder does nothing until .build() is called"]
pub struct ClientBuilder { base_url: String, timeout: Duration, retries: u32, user_agent: Option<String> }

impl Client {
    pub fn builder(base_url: impl Into<String>) -> ClientBuilder {
        ClientBuilder { base_url: base_url.into(), timeout: Duration::from_secs(30), retries: 3, user_agent: None }
    }
}

impl ClientBuilder {
    pub fn timeout(mut self, t: Duration) -> Self { self.timeout = t; self }
    pub fn retries(mut self, n: u32) -> Self { self.retries = n; self }
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self { self.user_agent = Some(ua.into()); self }

    pub fn build(self) -> Result<Client, BuildError> {
        if self.base_url.is_empty() { return Err(BuildError::MissingBaseUrl); }
        Ok(Client {
            base_url: self.base_url,
            timeout: self.timeout,
            retries: self.retries,
            user_agent: self.user_agent.unwrap_or_else(|| "my-crate/1.0".into()),
        })
    }
}

// let client = Client::builder("https://api.example.com").timeout(Duration::from_secs(5)).build()?;
```

Guidelines: required values go in `builder(...)`; optional ones get chainable setters; `build()` validates and returns `Result` if anything can be wrong. Consuming (`self`) setters suit one-shot builders; `&mut self` setters suit builders reused in loops. For many builders, `bon` or `typed-builder` derive them; use a typestate builder only when forgetting a field must be a compile error (see `type-design.md`).

For plain configuration with sensible defaults, a public options struct with `Default` is often simpler than a builder:

```rust
let opts = RenderOptions { width: 800, ..Default::default() };
```

(Mark such structs `#[non_exhaustive]` only if you don't want callers to construct them this way — it forbids struct-literal syntax outside your crate.)

## 4. Accepting arguments

Generic arguments make call sites pleasant, but each generic parameter monomorphizes the function and makes signatures harder to read. Use them where they pay off:

| Signature | Use when |
| --- | --- |
| `name: impl Into<String>` | you will *store* an owned `String` (constructors, setters). Callers pass `&str` or `String` without `.to_string()`. |
| `path: impl AsRef<Path>` | you only need to *read* a path (`fs::read` does this). |
| `items: impl IntoIterator<Item = T>` | you iterate once. |
| `s: &str`, `v: &[T]` | you only read — simplest and non-generic; prefer it for most functions. |
| `f: impl Fn(&T) -> bool` | callbacks; use `&dyn Fn` if the function is large and called with many closure types. |

```rust
// ❌ forces callers to allocate: f("x".to_string())
pub fn set_name(&mut self, name: String) { self.name = name; }
// ✅
pub fn set_name(&mut self, name: impl Into<String>) { self.name = name.into(); }
```

Don't make *everything* generic: `fn len_of(s: impl AsRef<str>)` buys nothing over `&str`. For large generic functions, the "inner function" trick avoids code bloat:

```rust
pub fn load(path: impl AsRef<Path>) -> io::Result<String> {
    fn inner(path: &Path) -> io::Result<String> { std::fs::read_to_string(path) }
    inner(path.as_ref())
}
```

Avoid boolean parameters at call sites (`render(true, false)`); use enums or an options struct.

## 5. Visibility: minimal surface

**Why:** everything `pub` is a promise. Private items can change freely; public ones need a major version bump.

- Start private. Promote to `pub(crate)` when other modules need it; to `pub` only when it is part of the crate's contract.
- Keep struct fields private unless the struct is plain data with no invariants (DTOs, config). Private fields let you add fields, change representation and enforce invariants later.
- `pub(super)` / `pub(in crate::path)` are fine for tightly coupled siblings.
- In binaries, `pub` vs `pub(crate)` matters less, but `unreachable_pub` lint keeps intent explicit.
- Don't leak dependency types in public signatures unless you mean to tie your semver to theirs (a public `reqwest::Error` in your API means upgrading `reqwest` is a breaking change for you). Wrap them in your own error/type.

## 6. Module layout and re-exports

- One concept per module; file `foo.rs` + directory `foo/` for submodules (the `mod.rs` style is legacy but acceptable — be consistent).
- Re-export the important items at the crate root so users write `my_crate::Client`, not `my_crate::client::http::inner::Client`:

```rust
// lib.rs
mod client;
mod error;
pub mod config;          // a public namespace users browse

pub use client::{Client, ClientBuilder};
pub use error::{Error, Result};
```

- Keep the internal module tree private and expose a flat, curated API; reorganize internals without breaking anyone.
- A `prelude` module is justified only for crates whose traits must be in scope to be useful (like `std::io::prelude`, `rayon::prelude`). Don't put every type in it; glob imports hide where names come from.
- A crate-level `pub type Result<T, E = Error> = std::result::Result<T, E>;` is idiomatic.

## 7. Implement the common traits

Users can't implement your crate's types' std traits themselves (orphan rule), so do it eagerly where it makes sense:

| Trait | Implement when |
| --- | --- |
| `Debug` | always, for every public type (`missing_debug_implementations` lint). Redact secrets manually. |
| `Clone` | the value can be meaningfully duplicated. |
| `Copy` | small, plain-data types (ids, small enums). Removing `Copy` later is breaking, so be sure. |
| `PartialEq`/`Eq` | equality is meaningful (`Eq` unless it holds floats). |
| `Hash` | `Eq` types used as keys; must agree with `Eq`. |
| `PartialOrd`/`Ord` | there is a natural order. |
| `Default` | there is an obvious empty/zero value. |
| `Display` | the value has a user-facing text form (errors, ids, enums). |
| `From`/`TryFrom`/`FromStr` | conversions exist. |
| `Serialize`/`Deserialize` | behind an optional `serde` feature in libraries. |
| `Send`/`Sync` | automatic — but check! An `Rc` or `RefCell` field silently makes your type `!Send`/`!Sync`, which breaks users on async runtimes. Add a compile-time assertion for important types. |

```rust
// Fails to compile if Client ever stops being Send + Sync
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
};
```

## 8. `#[must_use]` and `#[non_exhaustive]`

`#[must_use]` warns when a return value is ignored. Put it on:
- pure functions whose only effect is the result (`fn with_suffix(&self, ..) -> Self`),
- builders and guard types (dropping a guard immediately is almost always a bug),
- types that represent pending work (futures and iterators already have it).

```rust
#[must_use]
pub fn normalized(&self) -> Self { /* ... */ }
```

`Result` is already `#[must_use]`. Clippy's `must_use_candidate` (pedantic) finds candidates.

`#[non_exhaustive]` on a public enum lets you add variants without a breaking change (callers must have a `_` arm); on a struct it prevents construction and exhaustive destructuring outside the crate, so you can add fields. Use it on error enums and config/event types that will grow. Don't use it on enums whose variants are a closed set by nature (`Ordering`, `Direction { Up, Down }`) — forcing a `_` arm there loses exhaustiveness checking for your users.

## 9. Documentation

Write docs for every public item; enable `#![warn(missing_docs)]` in libraries. A good doc comment has a one-line summary, then details, then standard sections:

```rust
/// Parses a duration like `"1h30m"` or `"45s"`.
///
/// Units `h`, `m` and `s` may be combined in descending order.
///
/// # Examples
///
/// ```
/// # use my_crate::parse_duration;
/// let d = parse_duration("1h30m")?;
/// assert_eq!(d.as_secs(), 5400);
/// # Ok::<(), my_crate::ParseError>(())
/// ```
///
/// # Errors
///
/// Returns [`ParseError::UnknownUnit`] if a unit other than `h`, `m`, `s` appears.
pub fn parse_duration(s: &str) -> Result<Duration, ParseError> { /* ... */ }
```

- `# Errors` for every function returning `Result`; `# Panics` for every function that can panic; `# Safety` for every `unsafe fn` (what the caller must guarantee).
- Examples are **doctests**: they compile and run under `cargo test`, so they never rot. Use `?` with a trailing `Ok::<(), E>(())`; hide setup lines with `# `.
- Link items with intra-doc links: ``[`Client::builder`]``.
- Crate root gets `//!` docs with an overview and a quick-start example; `#![doc = include_str!("../README.md")]` keeps README and docs in sync (and doctests the README).
- Document *why* and *what invariants hold*, not what the code obviously does.

## 10. Feature flags

- **Features must be additive.** Enabling a feature may add API, never remove or change it — Cargo unifies features across the dependency graph, so some other crate may turn yours on.
- Don't create `no-foo` features; make `foo` default-on and let users disable default features.
- Gate optional integrations: `serde = ["dep:serde"]`, then `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]`.
- Keep `default` features minimal for libraries; heavy dependencies should be opt-in.
- Test the feature matrix in CI (`cargo hack --feature-powerset check`).
- Document features with `#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]` so docs.rs shows the badge.

## 11. Semver hazards

Things that are breaking changes even if they look small:
- adding a variant to an exhaustive public enum, or a public field to a struct without `#[non_exhaustive]` (callers with struct literals or exhaustive matches break),
- removing a trait impl, or adding a blanket impl,
- a type losing `Send`/`Sync`/`Copy`/`Unpin` because of a new field,
- tightening a generic bound, changing a function's argument types,
- bumping the major version of a dependency whose types appear in your public API,
- raising MSRV (treated as minor by convention, but announce it).

Use `cargo-semver-checks` before releases.

## 12. Pitfalls

- **Exposing `pub` fields on a type with invariants** — users will construct invalid values. Use a constructor.
- **`get_` prefixes and `to_` methods that are free** confuse readers about cost.
- **Returning `&Vec<T>` / `&String`** — return `&[T]` / `&str` so you can change the storage.
- **Taking `String` where `&str` suffices**, or `impl Into<String>` where you don't store it.
- **`impl Trait` in argument position** prevents turbofish calls (`f::<T>()`) — use a named generic if callers may need it.
- **Panicking in library code for bad input** — return an error; reserve panics for broken invariants (bugs).
- **Huge prelude/glob re-exports** cause name collisions in user code.
