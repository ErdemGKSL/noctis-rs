# Event pipeline baseline (Story 16.1)

The numbers below are the baseline of `crates/noctis-core/benches/pipeline.rs`:
the path from a shard's parsed `Inbound` to a handler start, before any Epic 16
optimization. Stories 16.2 and 16.3 compare against these numbers and update
this file.

**Not a gate.** No number here gates CI or the story loop: no gate runs the
bench and `benches/budgets.toml` has no `[pipeline]` table. Story 14.3 sets
budgets; Stories 16.2/16.3 compare against these numbers.

## Machine

- CPUs: 4 (`nproc`), Intel(R) Xeon(R) Processor @ 2.10GHz (`/proc/cpuinfo`)
- rustc: `rustc 1.97.0 (2d8144b78 2026-07-07)`
- OS: Ubuntu 24.04.5 LTS, Linux 6.18.44 (cloud container, shared host)

## Commit

`6d589d543dedfab8b5c81ba2685dabd2e9e3488c`, plus Story 16.1's bench files;
pipeline code unchanged (the only runtime change is the hidden, test-only lane
tap, which production Apps never install).

## Command

```sh
cargo bench -p noctis-core --bench pipeline
```

(`cargo bench` has no `--release`; the bench profile inherits release.)
`cargo bench -p noctis-core --bench pipeline -- --test` runs every scenario
once with smoke counts.

## Columns

- **items**: measured items (handled and unhandled). Saturation rows sum every
  run criterion made after its 1 s warm-up; fixed-rate rows count the 5
  measured seconds after a 1 s warm-up.
- **events/s**: items ÷ (first measured push → last measured handler start),
  summed over runs.
- **p50 / p99 / p99.9 µs**: push instant (`received_at`) → handler start of
  every **event** handler start; nearest rank, integer math; `n/a` below 100
  (p99) or 1000 (p99.9) samples. Scenario (c) excludes the slow guild's items.
- **int. p50 / int. p99 µs**: the same for **interaction** handler starts
  (scenarios (a) and (d)).
- **push ns/item**: producer time inside its push loop (stamp `received_at`,
  record the instant, `InboundSink::push`) ÷ pushed items, saturation only.
- **notes**: saturation runs × items per run; fixed-rate achieved push rate
  and the largest lag behind the schedule.

## Results

### (a) AD-25 mix, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 460000 | 89572 | 155979.1 | 616816.9 | 643011.3 | 50726.2 | 375061.5 | 258 | 23 runs × 20000 items |
| 4 | 460000 | 101232 | 138763.3 | 437668.8 | 450093.6 | 49921.3 | 258234.0 | 1871 | 23 runs × 20000 items |
| 16 | 400000 | 88393 | 155416.1 | 514377.4 | 523947.5 | 43901.6 | 353831.4 | 1249 | 20 runs × 20000 items |

### (a) AD-25 mix at a fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 9999 | 89.2 | 291.6 | 1575.5 | 153.4 | 497.4 | — | achieved 10000/s, max lag 9834.4 µs |
| 4 | 50000 | 9997 | 25.6 | 159.0 | 302.2 | 91.7 | 266.8 | — | achieved 9997/s, max lag 2938.8 µs |
| 16 | 50000 | 10001 | 19.7 | 175.7 | 403.6 | 72.8 | 234.5 | — | achieved 10001/s, max lag 3554.1 µs |

### (b) `MESSAGE_DELETE`, 1 concurrent no-op handler, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2220000 | 284365 | 34738.2 | 78206.1 | 85869.1 | — | — | 117 | 111 runs × 20000 items |
| 4 | 2240000 | 275313 | 36044.7 | 70315.2 | 80376.9 | — | — | 1475 | 112 runs × 20000 items |
| 16 | 2240000 | 268244 | 36501.9 | 71147.4 | 84614.6 | — | — | 1417 | 112 runs × 20000 items |

### (b) `MESSAGE_DELETE`, 4 concurrent no-op handlers, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1200000 | 200687 | 49562.5 | 100531.7 | 109476.6 | — | — | 115 | 60 runs × 20000 items |
| 4 | 1220000 | 190438 | 52173.8 | 100244.8 | 109863.8 | — | — | 1542 | 61 runs × 20000 items |
| 16 | 1200000 | 187111 | 52684.6 | 99852.8 | 107498.6 | — | — | 1548 | 60 runs × 20000 items |

### (c) ordered handler, 1 guild, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10000 | 1811.3 | 50475.3 | 51845.0 | — | — | — | achieved 10000/s, max lag 2964.9 µs |
| 4 | 50000 | 9999 | 1775.3 | 50013.9 | 51606.2 | — | — | — | achieved 9999/s, max lag 26862.7 µs |
| 16 | 50000 | 9998 | 1766.1 | 50402.1 | 51983.7 | — | — | — | achieved 9998/s, max lag 3268.4 µs |

### (c) ordered handler, 64 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10001 | 43.3 | 19319.1 | 47897.5 | — | — | — | achieved 10001/s, max lag 3961.5 µs |
| 4 | 50000 | 9998 | 14.5 | 19098.1 | 48051.2 | — | — | — | achieved 9998/s, max lag 3283.7 µs |
| 16 | 50000 | 9997 | 3.4 | 18776.3 | 47689.2 | — | — | — | achieved 9997/s, max lag 3731.9 µs |

### (c) ordered handler, 10 000 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10000 | 39.0 | 135.2 | 45200.1 | — | — | — | achieved 10000/s, max lag 3374.3 µs |
| 4 | 50000 | 9999 | 14.5 | 77.7 | 45401.9 | — | — | — | achieved 10000/s, max lag 2969.8 µs |
| 16 | 50000 | 9999 | 3.4 | 72.3 | 44982.0 | — | — | — | achieved 9999/s, max lag 3411.3 µs |

### (d) flood with 1 % interactions, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2460000 | 562453 | 20286.1 | 38444.9 | 49431.6 | 3613.0 | 8820.9 | 177 | 123 runs × 20000 items |
| 4 | 2500000 | 445161 | 24612.2 | 39882.7 | 47028.5 | 6471.6 | 9949.1 | 1765 | 125 runs × 20000 items |
| 16 | 2500000 | 411379 | 25735.7 | 57922.4 | 78677.8 | 4509.1 | 12535.8 | 1429 | 125 runs × 20000 items |

## Allocation counts (`tests/pipeline_alloc.rs`)

`cargo test --release -p noctis-core --test pipeline_alloc`: heap blocks and
bytes from `InboundSink::push` to the handlers' starts (min of 7 windows after
3 warm-ups, one-worker runtime, `DiscordCacheConfig::none()`). The block counts
are the test's `RECORDED` constants.

| Case | Blocks | Bytes |
| --- | ---: | ---: |
| `unhandled` (`MESSAGE_CREATE`, no handler) | 0 | 0 |
| `one_concurrent` (`MESSAGE_DELETE`, 1 concurrent handler) | 7 | 4496 |
| `two_concurrent` (`MESSAGE_REACTION_REMOVE_ALL`, 2 concurrent handlers) | 12 | 9576 |
| `ordered` (`CHANNEL_PINS_UPDATE`, 1 ordered handler) | 4 | 256 |

## Method notes

- **Injection (D2).** One `TestApp` per scenario (on `Runtime::for_test()`,
  reused across P and runs). Producers push into `TestApp::lane_sender()`, the
  same `LaneSender` the App's shard tasks push into (hidden
  `__testing::tap_lanes` seam). Every `Box<Event>` and
  `(Box<Interaction>, RawJson)` is built before timing; a producer only stamps
  `received_at = Instant::now()`, records that instant and calls `push`.
  Producer `i` of P uses `ShardId::new(i, P)`. Handlers take `Instant::now()`
  as their first statement.
- **criterion (D3).** Saturation runs are criterion benchmarks (`iter_custom`,
  `Throughput::Elements(20 000)`, 10 samples, 1 s warm-up, 5 s measurement);
  criterion's own time/throughput lines are in the run's log. Fixed-rate runs
  and every percentile are the bench's own code; the table above is what it
  prints at the end.
- **Lanes (D4).** Saturation Apps use `event_lane(262_144, 16_384)`, so a run of
  20 000 items never drops; fixed-rate Apps keep the default lane.
- **Interactions (D5).** The component (`Tap`, a button whose Payload is the
  item's seq) acks with `defer_update`, the command (`/seq`) with
  `defer_reply`; the TestApp HTTP fake answers the callbacks `204`, and its
  request log is cleared after every run. The testkit's run observer stays
  installed (a small constant overhead). Under saturation the interaction
  callbacks (3 000 per run in (a)) compete with the drain for the 4 CPUs, which
  is why (a) saturates far below (b) and (d).
- **Allocations (D6).** See the test's module docs.
- **Saturation latency** is queueing delay: all 20 000 items are pushed at once,
  so p50 grows with the backlog the drain works through. It is the number to
  compare between stories, not a production latency.
- **Pacing.** Item `seq` is due at `t0 + seq / 10 000 s` (`sleep_until`, every
  overdue item pushed at once); tokio's 1 ms timer makes pushes arrive in
  bursts of about 10. Achieved rate and max lag are reported, not fatal.
- **Ordered scenario (c).** Item k goes to guild `(k mod G) + 1`, every 1000th
  item to the slow guild, whose handler sleeps 50 ms. All items of a guild come
  from one producer (one shard per guild: guild g → producer `(g − 1) mod P`,
  the slow guild → guild 1's producer), so per-guild order is defined; the
  bench checks that per-guild starts are in seq order (0 violations in every
  row). With G = 1 only one producer has items, whatever P is. The slow guild
  is the smallest id above 10 000 in guild 1's 16.1-era sub-queue
  (`(id · 0x9E37_79B9_7F4A_7C15) >> 58`), so the p99/p99.9 of the 1- and
  64-guild rows show the head-of-line blocking of the 64 fixed hashed
  sub-queues that Story 16.2 removes.
- **Run checks.** A run fails (exit 2, naming scenario and P) on a start of an
  unknown seq, a duplicate start, an expected start missing after 30 s, or an
  order violation. Every producer's last item is a handled `MESSAGE_DELETE`, so
  the run end covers trailing unhandled items.
