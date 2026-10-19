# Testing

How to write Rust tests that are fast, deterministic, readable, and actually catch regressions.

**Contents**
1. Where tests live (unit, integration, doc)
2. Naming and structure
3. Tests that return `Result`
4. Panics and `#[should_panic]`
5. Table-driven tests (arrays, `macro_rules!`, `rstest`)
6. Property tests with `proptest`
7. Snapshot tests with `insta`
8. Async tests
9. Test seams: traits and fakes over heavy mocking
10. Filesystem and environment: `tempfile`, determinism
11. Serde round-trips
12. Running tests: `cargo nextest`, coverage
13. Pitfalls

---

## 1. Where tests live

| Kind | Location | Sees | Use for |
| --- | --- | --- | --- |
| Unit | `#[cfg(test)] mod tests` in the same file | private items | logic of one module |
| Integration | `tests/*.rs` | only the public API | how a user of the crate sees it |
| Doc | `///` code blocks on public items | public API | examples that must stay correct |

**Unit tests** sit next to the code so they move with it and can test private helpers.
`#[cfg(test)]` keeps them (and test-only imports) out of the normal build.

```rust
pub fn slugify(input: &str) -> String {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_collapses_separators() {
        assert_eq!(slugify("Hello,  World!"), "hello-world");
    }
}
```

**Integration tests** each compile as a separate crate. Every file in `tests/` becomes its
own test binary, which is slow to link when there are many; group related tests into a
few files (or one `tests/it/main.rs` with submodules) for large suites.

Shared helpers go in `tests/common/mod.rs` — *not* `tests/common.rs`, which Cargo would
treat as its own (empty) test binary.

```text
tests/
├── common/
│   └── mod.rs        // pub fn setup() -> TestApp { ... }
├── api.rs            // mod common; use common::setup;
└── storage.rs
```

A binary crate cannot be integration-tested directly. Keep `main.rs` thin and put the
logic in `lib.rs` so `tests/` can import it (see `tooling-and-project-layout.md`).

**Doctests** are compiled and run by `cargo test`. They document *and* verify. Use `?` in
them by ending with an `Ok` value; hide setup lines with `#`.

````rust
/// Parses a `key=value` pair.
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let (k, v) = mycrate::parse_pair("port=8080")?;
/// assert_eq!((k, v), ("port", "8080"));
/// # Ok(())
/// # }
/// ```
pub fn parse_pair(s: &str) -> Result<(&str, &str), ParseError> { /* ... */ }
````

Use ` ```no_run ` for examples that touch the network, ` ```compile_fail ` to document what
the type system forbids, and avoid ` ```ignore ` — ignored examples silently rot.

## 2. Naming and structure

Name tests after the behavior, not the function: `rejects_empty_username`,
`retries_three_times_then_gives_up`. The test name is the failure message you will read
in CI. Follow arrange / act / assert, one behavior per test.

```rust
// ❌ tells you nothing when it fails
#[test]
fn test1() { /* ... */ }

// ✅
#[test]
fn expired_token_is_rejected() { /* ... */ }
```

Prefer `assert_eq!(actual, expected)` over `assert!(a == b)`: on failure it prints both
values. Add a message when the values alone don't explain the case:
`assert_eq!(got, want, "input = {input:?}")`. For floats, compare with a tolerance.
`pretty_assertions` gives readable diffs for large structs.

## 3. Tests that return `Result`

A test can return `Result<(), E>` where `E: Debug`. This lets you use `?` instead of
`unwrap()` on every line, and the failure message points at the error value.

```rust
#[test]
fn reads_config() -> Result<(), Box<dyn std::error::Error>> {
    let cfg: Config = toml::from_str("port = 8080")?;
    assert_eq!(cfg.port, 8080);
    Ok(())
}
```

`unwrap()`/`expect()` are fine in tests — a panic *is* a test failure. Use whichever reads
better; `?` shines when there are many fallible steps.

## 4. Panics and `#[should_panic]`

Use `#[should_panic]` sparingly. It passes for *any* panic, including an unrelated
`unwrap` on line 1. Always pin it with `expected = "..."`. If the function returns
`Result`, assert on the error instead — that's more precise and doesn't rely on panics.

```rust
// ❌ passes even if the panic comes from somewhere else
#[test]
#[should_panic]
fn bad() { Percent::new(150); }

// ✅ assert on the returned error
#[test]
fn percent_over_100_is_rejected() {
    assert!(matches!(Percent::try_from(150), Err(PercentError::OutOfRange(150))));
}
```

## 5. Table-driven tests

When many inputs exercise the same logic, write a table instead of copy-pasting tests.

**Plain array** — zero dependencies, but stops at the first failing row, so include the
input in the message:

```rust
#[test]
fn parses_durations() {
    let cases = [("1s", 1), ("2m", 120), ("1h", 3600)];
    for (input, secs) in cases {
        assert_eq!(parse_duration(input).unwrap().as_secs(), secs, "input = {input:?}");
    }
}
```

**`macro_rules!`** — one real `#[test]` per row, so every failing case is reported and
can be run individually:

```rust
macro_rules! duration_cases {
    ($($name:ident: $input:expr => $secs:expr,)*) => {$(
        #[test]
        fn $name() {
            assert_eq!(parse_duration($input).unwrap().as_secs(), $secs);
        }
    )*};
}

duration_cases! {
    seconds: "1s" => 1,
    minutes: "2m" => 120,
    hours:   "1h" => 3600,
}
```

**`rstest`** — the same thing plus fixtures, without writing the macro yourself:

```rust
use rstest::rstest;

#[rstest]
#[case("1s", 1)]
#[case("2m", 120)]
#[case("1h", 3600)]
fn parses_durations(#[case] input: &str, #[case] secs: u64) {
    assert_eq!(parse_duration(input).unwrap().as_secs(), secs);
}
```

## 6. Property tests with `proptest`

Example tests check the cases you thought of; property tests check invariants against
hundreds of generated inputs and *shrink* failures to a minimal counterexample. Good
properties: round-trips (`decode(encode(x)) == x`), idempotence (`f(f(x)) == f(x)`),
invariants (`sorted output is sorted and a permutation of input`), and "never panics".

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn slugify_is_idempotent(s in ".*") {
        let once = slugify(&s);
        prop_assert_eq!(slugify(&once), once);
    }

    #[test]
    fn slug_has_no_uppercase(s in "[a-zA-Z0-9 ,.!]{0,64}") {
        prop_assert!(!slugify(&s).chars().any(char::is_uppercase));
    }
}
```

Commit the `proptest-regressions/` directory: it stores failing seeds so a once-found bug
is re-checked on every run. For parsers and anything handling untrusted bytes, also
consider fuzzing with `cargo fuzz`.

## 7. Snapshot tests with `insta`

For large or structured output (rendered text, JSON, ASTs, error messages), hand-written
expected values are tedious and drift. `insta` records the output to a file the first
time and diffs against it afterwards; `cargo insta review` accepts changes interactively.

```rust
#[test]
fn renders_report() {
    let report = render_report(&sample_data());
    insta::assert_snapshot!(report);
}

#[test]
fn serializes_user() {
    insta::assert_json_snapshot!(User { id: 1, name: "ada".into() });
}
```

Review snapshot diffs as carefully as code — blindly accepting them turns the test into
a no-op. Redact unstable fields (timestamps, UUIDs) with `insta`'s redactions.

## 8. Async tests

Use the runtime's test attribute. `#[tokio::test]` builds a current-thread runtime per
test; ask for `flavor = "multi_thread"` only when the code under test needs it.

```rust
#[tokio::test]
async fn fetches_user() {
    let repo = InMemoryRepo::default();
    repo.insert(User { id: 1, name: "ada".into() }).await;
    assert_eq!(repo.get(1).await.unwrap().name, "ada");
}
```

For time-dependent code, `#[tokio::test(start_paused = true)]` with `tokio::time::advance`
makes timeouts and retries instant and deterministic (needs tokio's `test-util` feature).

## 9. Test seams: traits and fakes over heavy mocking

Code that talks to the outside world (HTTP, DB, clock, randomness) is easiest to test
when that dependency sits behind a small trait. In tests, pass a hand-written fake. Fakes
are plain Rust: readable, refactor-safe, and they test behavior rather than call order.

```rust
pub trait Clock {
    fn now(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

pub fn is_expired(clock: &impl Clock, expires_at: u64) -> bool {
    clock.now() >= expires_at
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn now(&self) -> u64 { self.0 }
    }

    #[test]
    fn token_expires_at_deadline() {
        assert!(is_expired(&FixedClock(100), 100));
        assert!(!is_expired(&FixedClock(99), 100));
    }
}
```

Reach for `mockall` (`#[automock]`) when you genuinely need to assert *interactions*
("called exactly once with X") or the trait is large. Over-mocking couples tests to the
implementation: every refactor breaks them without any behavior changing.

For HTTP clients, `wiremock` runs a real local server; for databases, prefer a real
throwaway instance (`sqlx::test`, testcontainers) over mocking SQL.

## 10. Filesystem, environment, determinism

Tests run **in parallel** by default, in threads of one process. Anything shared —
files at fixed paths, env vars, the current directory, global state, ports — causes
flaky tests.

- Use `tempfile::tempdir()` for files; it's unique per test and deleted on drop.
- Don't mutate env vars in tests (`std::env::set_var` is `unsafe` in edition 2024 for
  exactly this reason). Pass config explicitly instead.
- Bind servers to port `0` and read back the assigned port.
- Seed randomness (`StdRng::seed_from_u64(42)`), inject the clock, and sort output from
  `HashMap` iteration before asserting.

```rust
#[test]
fn writes_and_reads_back() -> std::io::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("data.txt");
    std::fs::write(&path, "hello")?;
    assert_eq!(std::fs::read_to_string(&path)?, "hello");
    Ok(())
} // `dir` is removed here
```

## 11. Serde round-trips

Wire formats are contracts. Test both directions and pin the exact representation, so a
renamed field or changed `#[serde(tag)]` is caught before it breaks clients.

```rust
#[test]
fn event_wire_format_is_stable() -> serde_json::Result<()> {
    let ev = Event::Joined { user_id: 7 };
    let json = serde_json::to_value(&ev)?;
    assert_eq!(json, serde_json::json!({ "type": "joined", "user_id": 7 }));
    assert_eq!(serde_json::from_value::<Event>(json)?, ev);
    Ok(())
}
```

Combine with `proptest` (`decode(encode(x)) == x` for arbitrary `x`) when the type is
complex, and test that invalid input is *rejected* (unknown variants, missing fields).

## 12. Running tests

- `cargo test` — everything: unit, integration, doc.
- `cargo test name_fragment` — filter; `-- --nocapture` to see `println!` output.
- `cargo nextest run` — faster, process-per-test isolation, better output, retries for
  known-flaky tests. Note it does not run doctests; run `cargo test --doc` too in CI.
- `cargo llvm-cov` — coverage (`--html` for a report). Use coverage to find untested
  branches, not as a target to game.
- `#[ignore = "needs network"]` for slow/external tests; run them with
  `cargo test -- --ignored` in a dedicated CI job.

## 13. Pitfalls

- **Testing private details through `pub`.** Don't widen visibility for tests; unit
  tests in the same module already see private items. `pub(crate)` at most.
- **Order-dependent tests.** Each test must set up its own state.
- **Sleeping to wait.** `thread::sleep` makes tests slow *and* flaky; wait on a channel,
  a condition, or use paused time.
- **Asserting on `Debug` strings or error text from other crates.** Match on variants
  (`matches!`) instead; text changes between versions.
- **Giant test fixtures inline.** Move them to `tests/fixtures/` and load with
  `include_str!`.
- **Only happy paths.** Most bugs live in the error branches: empty input, boundaries,
  invalid UTF-8, duplicates, overflow.
