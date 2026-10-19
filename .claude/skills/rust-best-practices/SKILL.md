---
name: rust-best-practices
description: >-
  Idiomatic, production-grade Rust: how to design types, traits, errors, serde data
  models, macros, async code, tests and crate layout the way experienced Rustaceans do.
  Use this skill WHENEVER you write, review, refactor, or explain Rust code — any .rs
  file, Cargo.toml, a new crate or workspace, a CLI, web service, Discord bot, library,
  or a port from another language to Rust — even if the user only says "write this in
  Rust", "fix this borrow checker error", "add a struct for this JSON", or "clean up
  this code" without mentioning best practices. Also use it when the user asks how to
  structure a Rust project, which crates to pick, or why something is (un)idiomatic.
---

# Rust Best Practices

This skill is a map, not a textbook. The body below gives the principles that apply to
nearly every piece of Rust you write, then points to focused reference files. Read only
the references the task actually touches — each one is self-contained.

## Core principles

These are the ideas everything else follows from. When two rules seem to conflict, fall
back on these.

1. **Make illegal states unrepresentable.** Push invariants into the type system so the
   compiler enforces them, instead of checking at runtime and hoping. A closed set of
   values is an `enum`, not a `String`. A validated value is a newtype whose only
   constructor validates. A value that may be missing is `Option<T>`, not a sentinel.
2. **Parse, don't validate.** Convert untrusted input (JSON, CLI args, env vars, user
   text) into strong types at the boundary, once. Inside the program, work only with the
   strong types. `serde`, `FromStr` and `TryFrom` are the tools for this.
3. **Borrow by default, own when you must.** Take `&str`, `&[T]`, `&T` or `impl AsRef<_>`
   in parameters; return owned values. Clone deliberately, not to silence the borrow
   checker — a borrow error usually means the data flow wants restructuring.
4. **Errors are values with types.** Libraries return a typed error enum
   (`thiserror`); applications aggregate with context (`anyhow`). No `unwrap()` on
   anything that can fail because of input, I/O or the environment.
5. **Lean on traits for flexibility, not inheritance.** Small traits, implemented for
   many types; generic functions bounded by what they actually use; standard
   conversion traits (`From`, `TryFrom`, `AsRef`, `Display`, `FromStr`, `Default`)
   instead of ad-hoc methods.
6. **Don't repeat yourself — but reach for functions and generics before macros.**
   Use `derive`, generics and traits first; use `macro_rules!` when the repetition is
   syntactic (many near-identical impls, enum ↔ string tables, test matrices).
7. **Let the tools work for you.** Code is `rustfmt`-formatted, `clippy`-clean
   (including `clippy::pedantic` where sensible), and the crate builds without warnings.
   Run `cargo fmt`, `cargo clippy --all-targets --all-features` and `cargo test` before
   calling work done.
8. **Make the obvious thing the fast thing.** Prefer iterators, slices and stack data;
   avoid needless allocation and `Box<dyn …>` in hot paths — but measure before
   micro-optimizing, and never trade clarity for a guess.

## Reference files — read what the task touches

| File | Read when you are… |
| --- | --- |
| `references/type-design.md` | modeling a domain: enums vs strings, newtypes, `Option`, typestate, builders, `#[non_exhaustive]` |
| `references/serde.md` | defining data that is (de)serialized: JSON/TOML/YAML structs, API payloads, config files, enum tagging, renames, defaults, custom (de)serializers |
| `references/traits-and-generics.md` | designing traits, `impl` blocks, generics vs `dyn`, conversion traits, extension traits, blanket impls, sealed traits |
| `references/error-handling.md` | writing anything that can fail: error enums, `thiserror`/`anyhow`, `?`, context, panics vs errors |
| `references/macros.md` | removing repetitive code with `macro_rules!`, deciding macro vs generic, writing or using derive/proc macros |
| `references/ownership-and-lifetimes.md` | fighting the borrow checker, choosing `&`/`Cow`/`Rc`/`Arc`, lifetimes in structs, interior mutability |
| `references/iterators-and-collections.md` | looping, transforming or collecting data; choosing `Vec`/`HashMap`/`BTreeMap`/`HashSet`; the entry API |
| `references/api-design.md` | writing a library or public module: naming, visibility, `#[must_use]`, module layout, documentation, feature flags, semver |
| `references/async-and-concurrency.md` | using `tokio`/async, threads, channels, `Mutex`/`RwLock`, shared state, cancellation, `Send`/`Sync` |
| `references/testing.md` | writing unit, integration, doc or property tests, snapshot tests, test helpers and fixtures |
| `references/performance.md` | optimizing, profiling, reducing allocations, choosing data layouts, benchmarking |
| `references/unsafe-and-ffi.md` | touching `unsafe`, raw pointers, FFI or `Send`/`Sync` impls |
| `references/tooling-and-project-layout.md` | creating a crate/workspace, configuring `Cargo.toml`, lints, clippy, rustfmt, CI, MSRV, dependency hygiene, picking common crates |

For a code review, also walk the checklist below.

## Workflow when writing Rust

1. **Model the data first.** Sketch the types (enums, structs, newtypes) before
   functions. Most of the design quality of a Rust program is decided here.
2. **Define the error type** for the module/crate before writing fallible functions.
3. **Write the functions against the types**, borrowing in parameters and returning
   owned results or `Result<T, E>`.
4. **Add tests next to the code** (`#[cfg(test)] mod tests`) and doc examples for public
   items.
5. **Run the toolchain**: `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`,
   `cargo test`. Fix warnings rather than `#[allow]`-ing them; when an `allow` is truly
   right, scope it narrowly and leave a one-line reason (or use `#[expect(lint, reason = "…")]`).

When the user's existing code base has established conventions (error crate, async
runtime, naming), follow them over the defaults in this skill and mention any concrete
risk you see instead of silently rewriting.

## Review checklist

Use this as a fast pass over any Rust code; each item links to the reference with detail.

- [ ] Closed sets of values are enums, not `String`/`&str`/integer codes (type-design)
- [ ] Validated values are newtypes with private fields and a fallible constructor (type-design)
- [ ] Serde structs use `rename_all`, `default`, `skip_serializing_if`, enum tagging instead of hand-written conversion code (serde)
- [ ] No `unwrap()`/`expect()` on fallible input or I/O outside tests and proven invariants; `expect` messages state the invariant (error-handling)
- [ ] Library errors are typed enums; application code adds context (error-handling)
- [ ] Parameters borrow (`&str`, `&[T]`, `impl AsRef<Path>`), not `&String`/`&Vec<T>`/owned-without-need (ownership)
- [ ] No `.clone()` used only to appease the borrow checker in a loop or hot path (ownership)
- [ ] Standard traits implemented where meaningful: `Debug` on all public types, `Display` for user-facing, `From`/`TryFrom`/`FromStr` for conversions, `Default` where a default exists (traits)
- [ ] Iterator chains instead of index loops and manual `push` (iterators)
- [ ] Repetitive impls are generated by a generic, blanket impl, derive, or `macro_rules!` (macros)
- [ ] No blocking calls or `std::sync::Mutex` guards held across `.await` (async)
- [ ] Every `unsafe` block has a `// SAFETY:` comment justifying it (unsafe)
- [ ] Public items are documented; functions returning values that must be used are `#[must_use]` (api-design)
- [ ] `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test` pass (tooling)

## Quick idiom table

| Instead of | Prefer | Why |
| --- | --- | --- |
| `status: String` with `"active"`/`"banned"` | `enum Status { Active, Banned }` | exhaustive `match`, no typos, no invalid states |
| `fn f(s: &String)` | `fn f(s: &str)` | accepts `String`, `&str`, literals |
| `fn f(v: &Vec<T>)` | `fn f(v: &[T])` | accepts arrays, vecs, slices |
| `if x.is_some() { x.unwrap() }` | `if let Some(v) = x` / `let Some(v) = x else { … }` | one check, no panic path |
| `match r { Ok(v) => v, Err(e) => return Err(e.into()) }` | `r?` | same semantics, less noise |
| manual `impl` per type, copy-pasted | generic, blanket impl, or `macro_rules!` | one place to fix bugs |
| `Box<dyn Error>` in a library API | `thiserror` enum | callers can match on variants |
| `for i in 0..v.len() { v[i] … }` | `for x in &v` / `.iter().enumerate()` | no bounds checks, no off-by-one |
| `.map(...).collect::<Vec<_>>().len()` | `.count()` / `.filter(...).count()` | no allocation |
| `bool` flags in signatures `f(true, false)` | small enums or an options struct | readable call sites |
| `Option<Option<T>>`, `Result<Option<T>, …>` puzzles | a dedicated enum | names the states |
| `pub` fields on validated types | private fields + constructor + getters | invariants can't be broken |
| `lazy_static!` / `once_cell::Lazy` | `std::sync::LazyLock` / `OnceLock` | in std since 1.80 |
