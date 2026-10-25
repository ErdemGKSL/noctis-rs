# Changelog

All notable changes to Noctis are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Before 1.0, a breaking change bumps the minor version (`0.x` → `0.(x+1)`) and
everything else bumps the patch version.

## [Unreleased]

### Performance budgets (PRD Q3)

The NFR-1, NFR-3 and NFR-9 targets were `[ASSUMPTION]`s until the first
benchmarks measured them (Story 14.3). Each one below is **confirmed** (the
measurement met the PRD target, which stays the budget) or **re-set** (the
budget becomes the measurement + 10 %). Every table of
`crates/noctis-core/benches/budgets.toml` now gates (`gate = true`): an
over-budget metric fails the bench.

- **NFR-1, framework memory per idle guild:** target ≤ 1 KiB (1024 bytes)
  per guild, excluding the Discord cache; measured 28.9 bytes per guild over
  10 000 idle guilds. **Confirmed** (`max = 1024`).
- **NFR-3, dispatch latency, events:** target p99 ≤ 1 ms from event arrival
  to handler start at 10 000 events/s on 4 cores (AD-25 mix); measured
  164.4 µs (median of three runs' p99: 191.9, 164.4, 162.0 µs).
  **Confirmed** (`max = 1000` µs).
- **NFR-3, dispatch latency, interactions:** the same target for the mix's
  component and command interactions; measured 284.7 µs (runs: 316.2,
  265.2, 284.7 µs). **Confirmed** (`max = 1000` µs).
- **NFR-3, reference card render:** target p50 ≤ 10 ms and p99 ≤ 50 ms
  (934×282 PNG, warm caches, saturated pool); measured p50 8.3 ms and p99
  20.0 ms. **Confirmed** (`p50_ms = 10.0`, `p99_ms = 50.0`).
- **NFR-9, Route lookup and fixed-width Payload decode:** target no heap
  allocation; measured 0 allocations (`noctis/tests/alloc_dispatch.rs` over
  10 and 10 000 Routes, `noctis-codec/tests/alloc_decode.rs`).
  **Confirmed** (exact pins).
- **NFR-9, per interaction:** target at most one Payload port call and one
  boxed handler future per interaction (AD-25); measured 0 `RefStore` calls
  for an inline click and a command, exactly 1 (`get`) for a `by_ref`
  click, and one handler-future allocation (a handler holding 16 KiB across
  an `.await` adds no block and 16 KiB of bytes). Push to handler start
  costs 12 heap blocks for an inline click, 16 for a `by_ref` click and 88
  for a slash command (`noctis-core/tests/interaction_alloc.rs`).
  **Confirmed** (exact pins).

Machine: 4 CPUs (`nproc`), Intel(R) Xeon(R) Processor @ 2.10GHz, rustc
1.97.0, Linux 6.18.44 (cloud container, shared host).

Commands:

```sh
cargo bench -p noctis-core --bench dispatch_latency
cargo bench -p noctis-core --bench memory_per_guild
cargo bench -p noctis-image --features render --bench reference_card
cargo test --release -p noctis-core --test interaction_alloc --test pipeline_alloc
cargo test --release -p noctis --test alloc_dispatch
cargo test --release -p noctis-codec --test alloc_decode
```

### Changed

- Component, command and autocomplete handler futures are boxed through a
  `map` combinator instead of an `async move` wrapper, which stored the
  handler's future twice inside its one allocation (about 1.4 KiB less heap
  per component interaction).
