# Event pipeline benchmarks

The numbers below are the baseline of `crates/noctis-core/benches/pipeline.rs`:
the path from a shard's parsed `Inbound` to a handler start, before any Epic 16
optimization. Stories 16.2 and 16.3 compare against these numbers and update
this file; Story 16.2's numbers follow the baseline in
[Story 16.2](#story-162-partitioned-lanes-parallel-drain-per-guild-fifos), and
Story 16.3's follow them in
[Story 16.3](#story-163-per-event-and-per-handler-overhead).

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
  run criterion made, except runs begun within 1 s of the scenario's first run
  (a fixed 1 s cut, not criterion's exact warm-up phase, which can last about
  2 s); fixed-rate rows count the 5
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

## Story 16.2: partitioned lanes, parallel drain, per-guild FIFOs

Same bench, unchanged measurement code, same command and columns as above.

### Machine

- CPUs: 4 (`nproc`), Intel(R) Xeon(R) Processor @ 2.10GHz (`/proc/cpuinfo`)
- rustc: `rustc 1.97.0 (2d8144b78 2026-07-07)`
- OS: Ubuntu 24.04.5 LTS, Linux 6.18.44 (cloud container, shared host)

### Commit

`571314f6697d7bda149761cbdbd387fdd3cae7d3` (lanes partitioned by shard, one
drain per partition with batched pops, on-demand per-guild ordered FIFOs).
Later review patches touched the drain and the lanes (interactions served
between batched events, overflow refilled into freed capacity); the numbers
were not re-measured for them.

The bench runtime has 4 worker threads, so the App runs **4 lane partitions**
and 4 drain tasks (partitions = runtime workers). Producer `i` of P pushes on
`ShardId::new(i, P)`, so P = 1 fills one partition, P = 4 and P = 16 fill all
four. Each drain yields to the runtime after every batch (up to 32 items):
without it the 4 drains, one per worker thread, delayed the handler and
interaction tasks they spawned, and (a) at P = 4/16 fell below the baseline
(49 861-75 203 events/s over two runs).

### Results

#### (a) AD-25 mix, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 640000 | 148608 | 114671.4 | 196802.7 | 206602.9 | 51661.8 | 129888.9 | 239 | 32 runs × 20000 items |
| 4 | 640000 | 137346 | 124040.9 | 173600.8 | 182974.1 | 47882.6 | 118612.0 | 623 | 32 runs × 20000 items |
| 16 | 660000 | 108038 | 155384.3 | 287834.4 | 320913.5 | 60158.3 | 182880.9 | 815 | 33 runs × 20000 items |

#### (a) AD-25 mix at a fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 9997 | 114.6 | 412.1 | 889.3 | 174.8 | 450.4 | — | achieved 9998/s, max lag 4920.8 µs |
| 4 | 50000 | 10000 | 23.3 | 165.2 | 340.4 | 100.1 | 299.2 | — | achieved 10000/s, max lag 9271.0 µs |
| 16 | 50000 | 9999 | 27.5 | 279.4 | 662.6 | 90.2 | 314.9 | — | achieved 10000/s, max lag 5017.4 µs |

#### (b) `MESSAGE_DELETE`, 1 concurrent no-op handler, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2220000 | 280423 | 35653.1 | 74824.6 | 85709.1 | — | — | 148 | 111 runs × 20000 items |
| 4 | 2400000 | 444298 | 21581.9 | 42902.2 | 48008.1 | — | — | 414 | 120 runs × 20000 items |
| 16 | 2440000 | 416899 | 22887.5 | 45364.0 | 67492.2 | — | — | 614 | 122 runs × 20000 items |

#### (b) `MESSAGE_DELETE`, 4 concurrent no-op handlers, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1220000 | 193446 | 52066.3 | 102649.5 | 109288.9 | — | — | 145 | 61 runs × 20000 items |
| 4 | 1180000 | 212149 | 46300.1 | 92732.8 | 98405.9 | — | — | 434 | 59 runs × 20000 items |
| 16 | 1180000 | 221607 | 44391.4 | 87722.9 | 91383.9 | — | — | 574 | 59 runs × 20000 items |

#### (c) ordered handler, 1 guild, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 9999 | 38.4 | 185.0 | 745.6 | — | — | — | achieved 9999/s, max lag 25560.7 µs |
| 4 | 50000 | 9999 | 36.1 | 90.4 | 452.6 | — | — | — | achieved 10000/s, max lag 6295.3 µs |
| 16 | 50000 | 10001 | 36.3 | 85.4 | 280.4 | — | — | — | achieved 10001/s, max lag 5804.7 µs |

#### (c) ordered handler, 64 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10000 | 67.2 | 141.1 | 242.3 | — | — | — | achieved 10000/s, max lag 3575.9 µs |
| 4 | 50000 | 9999 | 19.1 | 87.3 | 194.2 | — | — | — | achieved 9999/s, max lag 4172.8 µs |
| 16 | 50000 | 9998 | 16.2 | 91.3 | 373.3 | — | — | — | achieved 9998/s, max lag 4165.6 µs |

#### (c) ordered handler, 10 000 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10001 | 66.4 | 135.4 | 592.8 | — | — | — | achieved 10001/s, max lag 3726.9 µs |
| 4 | 50000 | 10000 | 18.5 | 89.5 | 225.0 | — | — | — | achieved 10000/s, max lag 5013.9 µs |
| 16 | 50000 | 9998 | 16.4 | 92.0 | 258.8 | — | — | — | achieved 9998/s, max lag 6741.6 µs |

#### (d) flood with 1 % interactions, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2480000 | 462160 | 25188.4 | 43961.9 | 49470.0 | 5150.3 | 10146.8 | 215 | 124 runs × 20000 items |
| 4 | 2480000 | 541774 | 21136.0 | 35842.6 | 42610.1 | 4879.1 | 9986.7 | 602 | 124 runs × 20000 items |
| 16 | 2480000 | 505042 | 21865.5 | 37256.9 | 41948.4 | 4388.5 | 7213.0 | 746 | 124 runs × 20000 items |

### Baseline vs 16.2

| Scenario | P | events/s base | events/s 16.2 | ratio | p99 µs base | p99 µs 16.2 | int. p99 µs base | int. p99 µs 16.2 | push ns base | push ns 16.2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| (a) mix, saturation | 1 | 89572 | 148608 | 1.66× | 616816.9 | 196802.7 | 375061.5 | 129888.9 | 258 | 239 |
| (a) mix, saturation | 4 | 101232 | 137346 | 1.36× | 437668.8 | 173600.8 | 258234.0 | 118612.0 | 1871 | 623 |
| (a) mix, saturation | 16 | 88393 | 108038 | 1.22× | 514377.4 | 287834.4 | 353831.4 | 182880.9 | 1249 | 815 |
| (a) mix @ 10k/s | 1 | 9999 | 9997 | 1.00× | 291.6 | 412.1 | 497.4 | 450.4 | — | — |
| (a) mix @ 10k/s | 4 | 9997 | 10000 | 1.00× | 159.0 | 165.2 | 266.8 | 299.2 | — | — |
| (a) mix @ 10k/s | 16 | 10001 | 9999 | 1.00× | 175.7 | 279.4 | 234.5 | 314.9 | — | — |
| (b) delete, 1 handler | 1 | 284365 | 280423 | 0.99× | 78206.1 | 74824.6 | — | — | 117 | 148 |
| (b) delete, 1 handler | 4 | 275313 | 444298 | 1.61× | 70315.2 | 42902.2 | — | — | 1475 | 414 |
| (b) delete, 1 handler | 16 | 268244 | 416899 | 1.55× | 71147.4 | 45364.0 | — | — | 1417 | 614 |
| (b) delete, 4 handlers | 1 | 200687 | 193446 | 0.96× | 100531.7 | 102649.5 | — | — | 115 | 145 |
| (b) delete, 4 handlers | 4 | 190438 | 212149 | 1.11× | 100244.8 | 92732.8 | — | — | 1542 | 434 |
| (b) delete, 4 handlers | 16 | 187111 | 221607 | 1.18× | 99852.8 | 87722.9 | — | — | 1548 | 574 |
| (c) ordered, 1 guilds @ 10k/s | 1 | 10000 | 9999 | 1.00× | 50475.3 | 185.0 | — | — | — | — |
| (c) ordered, 1 guilds @ 10k/s | 4 | 9999 | 9999 | 1.00× | 50013.9 | 90.4 | — | — | — | — |
| (c) ordered, 1 guilds @ 10k/s | 16 | 9998 | 10001 | 1.00× | 50402.1 | 85.4 | — | — | — | — |
| (c) ordered, 64 guilds @ 10k/s | 1 | 10001 | 10000 | 1.00× | 19319.1 | 141.1 | — | — | — | — |
| (c) ordered, 64 guilds @ 10k/s | 4 | 9998 | 9999 | 1.00× | 19098.1 | 87.3 | — | — | — | — |
| (c) ordered, 64 guilds @ 10k/s | 16 | 9997 | 9998 | 1.00× | 18776.3 | 91.3 | — | — | — | — |
| (c) ordered, 10000 guilds @ 10k/s | 1 | 10000 | 10001 | 1.00× | 135.2 | 135.4 | — | — | — | — |
| (c) ordered, 10000 guilds @ 10k/s | 4 | 9999 | 10000 | 1.00× | 77.7 | 89.5 | — | — | — | — |
| (c) ordered, 10000 guilds @ 10k/s | 16 | 9999 | 9998 | 1.00× | 72.3 | 92.0 | — | — | — | — |
| (d) flood, 1 % interactions | 1 | 562453 | 462160 | 0.82× | 38444.9 | 43961.9 | 8820.9 | 10146.8 | 177 | 215 |
| (d) flood, 1 % interactions | 4 | 445161 | 541774 | 1.22× | 39882.7 | 35842.6 | 9949.1 | 9986.7 | 1765 | 602 |
| (d) flood, 1 % interactions | 16 | 411379 | 505042 | 1.23× | 57922.4 | 37256.9 | 12535.8 | 7213.0 | 1429 | 746 |

Push cost P = 16 / P = 1 for (a) saturation: baseline 1249 / 258 = 4.84×,
16.2 815 / 239 = 3.41×. Across three full runs during the story the P = 1
push cost of the saturation rows varied from 145 to 294 ns/item.

### Allocation counts (`tests/pipeline_alloc.rs`)

| Case | Blocks before | Bytes before | Blocks 16.2 | Bytes 16.2 |
| --- | ---: | ---: | ---: | ---: |
| `unhandled` | 0 | 0 | 0 | 0 |
| `one_concurrent` | 7 | 4496 | 7 | 4496 |
| `two_concurrent` | 12 | 9576 | 12 | 9576 |
| `ordered` | 4 | 256 | 5 | 3328 |

`ordered` gains one block: an ordered event of a guild without queued work
spawns that guild's runner task (the 16.1 code reused one of 64 long-lived
sub-queue tasks and an mpsc block).

### Targets

| Target | Measured | Verdict |
| --- | --- | --- |
| (a) saturation events/s at P = 16 ≥ 2 × 88 393 = 176 786 | 108 038 (1.22×) | missed |
| (a) saturation push ns/item P = 16 ≤ 1.5 × P = 1 | 815 / 239 = 3.41× (baseline 4.84×) | missed |
| (a) fixed 10k/s p99 ≤ 1 ms (NFR-3) | 412.1 / 165.2 / 279.4 µs | met |
| (d) int. p99 ≤ baseline at every P | P1 10 146.8 > 8 820.9; P4 9 986.7 > 9 949.1; P16 7 213.0 < 12 535.8 | missed at P = 1 and P = 4 |
| (c) 64 guilds p99 ≤ 1 ms, 0 order violations | 141.1 / 87.3 / 91.3 µs, 0 violations | met |

Causes of the missed targets (recorded, not gating, per the epic):

- **(a) throughput.** On 4 CPUs the 4 drains, the producers, the handler and
  interaction tasks and the HTTP fake that acks 3 000 interactions per run share
  the same 4 worker threads, so (a) is bound by handler and interaction CPU
  work, not by the lane lock: the lane-bound scenario (b) with one handler
  gains 1.61× / 1.55× at P = 4 / 16. 16.2 lifts (a) 1.22×-1.66× over the
  baseline at every P, short of 2× at P = 16.
- **Push ratio.** The P = 16 push cost fell from 1249 to 815 ns/item, but 16
  producers share 4 partitions (4 per lock), every event push and pop updates
  the one global event total that keeps the bound global, and the producers run
  on the worker threads that also run the drains. The P = 1 denominator is
  itself noisy (145-294 ns/item between runs).
- **(d) interaction p99.** With one partition (P = 1) the per-batch yield adds
  a scheduler round trip per 32 items; at P = 4 the value is within 0.4 % of
  the baseline. The run before the yield measured 8 878.0 / 15 802.2 /
  12 261.3 µs: on this shared host the P = 1 and P = 4 values move by more than
  the margin between runs.

## Story 16.3: per-event and per-handler overhead

Same bench, unchanged measurement code, same command and columns as above.

### Machine

- CPUs: 4 (`nproc`), Intel(R) Xeon(R) Processor @ 2.10GHz (`/proc/cpuinfo`)
- rustc: `rustc 1.97.0 (2d8144b78 2026-07-07)`
- OS: Ubuntu 24.04.5 LTS, Linux 6.18.44 (cloud container, shared host)

### Commit

`57277cc4cd22870ed1140353eb6b90601c7274a1` plus Story 16.3's changes (measured on the working tree that became the
Story 16.3 commit): cached event metric handles (handler duration and errors per
App, dropped events per event type), the payload filter checked before any
`Event` clone, one concurrent handler awaited inline and several joined through
one allocation, no Middleware state without event Middleware, a shared locale
cell per guild record without a resolver or event Middleware, and no span
attach or entry for a disabled span. 4 lane partitions, as in Story 16.2.

### Results

#### (a) AD-25 mix, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 640000 | 148722 | 116072.4 | 140614.5 | 145779.5 | 50879.7 | 100544.4 | 243 | 32 runs × 20000 items |
| 4 | 640000 | 136814 | 124622.9 | 193061.9 | 201550.8 | 47300.5 | 120280.3 | 577 | 32 runs × 20000 items |
| 16 | 640000 | 124834 | 135140.6 | 183491.1 | 192623.0 | 49446.3 | 131913.9 | 669 | 32 runs × 20000 items |

#### (a) AD-25 mix at a fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 9997 | 103.5 | 903.9 | 5104.5 | 165.1 | 758.4 | — | achieved 9999/s, max lag 32421.0 µs |
| 4 | 50000 | 9999 | 20.5 | 162.7 | 480.5 | 96.0 | 281.4 | — | achieved 10000/s, max lag 5411.5 µs |
| 16 | 50000 | 9998 | 28.6 | 327.6 | 2014.1 | 94.8 | 383.2 | — | achieved 9998/s, max lag 8285.4 µs |

#### (b) `MESSAGE_DELETE`, 1 concurrent no-op handler, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2240000 | 269071 | 36697.0 | 82236.7 | 92712.0 | — | — | 146 | 112 runs × 20000 items |
| 4 | 2440000 | 437158 | 21722.8 | 43249.0 | 47678.9 | — | — | 447 | 122 runs × 20000 items |
| 16 | 2440000 | 427305 | 22449.9 | 44227.9 | 49453.7 | — | — | 639 | 122 runs × 20000 items |

#### (b) `MESSAGE_DELETE`, 4 concurrent no-op handlers, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 1200000 | 190638 | 52465.7 | 107111.3 | 118471.1 | — | — | 147 | 60 runs × 20000 items |
| 4 | 2260000 | 231029 | 42529.3 | 84882.4 | 93303.9 | — | — | 443 | 113 runs × 20000 items |
| 16 | 2260000 | 229851 | 42762.8 | 84599.5 | 88787.9 | — | — | 577 | 113 runs × 20000 items |

#### (c) ordered handler, 1 guild, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10001 | 34.3 | 88.7 | 365.1 | — | — | — | achieved 10001/s, max lag 11500.6 µs |
| 4 | 50000 | 10001 | 34.2 | 89.9 | 352.3 | — | — | — | achieved 10001/s, max lag 7451.1 µs |
| 16 | 50000 | 9999 | 36.9 | 104.4 | 829.5 | — | — | — | achieved 9999/s, max lag 5537.5 µs |

#### (c) ordered handler, 64 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 10000 | 67.7 | 174.1 | 1266.6 | — | — | — | achieved 10000/s, max lag 5074.3 µs |
| 4 | 50000 | 9998 | 19.3 | 90.6 | 231.5 | — | — | — | achieved 9999/s, max lag 6675.1 µs |
| 16 | 50000 | 10001 | 17.1 | 169.0 | 996.0 | — | — | — | achieved 10001/s, max lag 34497.9 µs |

#### (c) ordered handler, 10 000 guilds, fixed 10 000 events/s

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 50000 | 9999 | 62.7 | 153.8 | 988.6 | — | — | — | achieved 10000/s, max lag 6211.3 µs |
| 4 | 50000 | 10001 | 18.6 | 85.7 | 151.2 | — | — | — | achieved 10001/s, max lag 3488.9 µs |
| 16 | 50000 | 9998 | 16.6 | 93.3 | 218.9 | — | — | — | achieved 9998/s, max lag 9054.9 µs |

#### (d) flood with 1 % interactions, saturation

| P | items | events/s | p50 µs | p99 µs | p99.9 µs | int. p50 µs | int. p99 µs | push ns/item | notes |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 2200000 | 432005 | 26730.9 | 46563.9 | 59104.3 | 5312.8 | 8615.2 | 219 | 110 runs × 20000 items |
| 4 | 2480000 | 534622 | 21482.3 | 35408.4 | 39492.1 | 5013.9 | 8420.1 | 631 | 124 runs × 20000 items |
| 16 | 2200000 | 464103 | 23808.7 | 40241.2 | 46248.5 | 4790.0 | 7946.0 | 830 | 110 runs × 20000 items |

### 16.2 vs 16.3

| Scenario | P | events/s 16.2 | events/s 16.3 | ratio | p99 µs 16.2 | p99 µs 16.3 | verdict (≥ 1.3×) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| (b) delete, 1 handler | 1 | 280423 | 269071 | 0.96× | 74824.6 | 82236.7 | missed |
| (b) delete, 1 handler | 4 | 444298 | 437158 | 0.98× | 42902.2 | 43249.0 | missed |
| (b) delete, 1 handler | 16 | 416899 | 427305 | 1.02× | 45364.0 | 44227.9 | missed |
| (b) delete, 4 handlers | 1 | 193446 | 190638 | 0.99× | 102649.5 | 107111.3 | missed |
| (b) delete, 4 handlers | 4 | 212149 | 231029 | 1.09× | 92732.8 | 84882.4 | missed |
| (b) delete, 4 handlers | 16 | 221607 | 229851 | 1.04× | 87722.9 | 84599.5 | missed |

### Allocation counts (`tests/pipeline_alloc.rs`)

| Case | Blocks before 16.3 | Bytes before 16.3 | Blocks 16.3 | Bytes 16.3 |
| --- | ---: | ---: | ---: | ---: |
| `unhandled` | 0 | 0 | 0 | 0 |
| `one_concurrent` | 7 | 4496 | 3 | 2688 |
| `two_concurrent` | 12 | 9576 | 6 | 6064 |
| `ordered` | 5 | 3328 | 3 | 3208 |

What is left per case: `one_concurrent` the job task, the `EventCtx` `Arc` and
the boxed handler future; `two_concurrent` the job task, the one `join_all`
slice and a context and a future per handler; `ordered` the guild's runner
task, a context and a future. Removed: the label `Vec` (and recorder lookup) of
each labeled `metrics::` macro, the per-run `LocaleCell` `Arc`, the `Vec` the
concurrent runs were collected into and `join_all`'s own copy for one handler.

### Targets

| Target | Measured | Verdict |
| --- | --- | --- |
| (b) 1 handler events/s ≥ 1.3 × 16.2 at P = 1 / 4 / 16 | 0.96× / 0.98× / 1.02× | missed |
| (b) 4 handlers events/s ≥ 1.3 × 16.2 at P = 1 / 4 / 16 | 0.99× / 1.09× / 1.04× | missed |
| dhat `unhandled` = 0 blocks | 0 | met |
| dhat `one_concurrent` ≤ 3 blocks | 3 | met |
| dhat counts below 7 / 12 / 5 (one / two concurrent, ordered) | 3 / 6 / 3 | met |
| (a) fixed 10k/s p99 ≤ 1 ms (NFR-3) | 903.9 / 162.7 / 327.6 µs | met |

Cause of the missed throughput target (recorded, not gating, per the epic):
scenario (b) at saturation is bound by scheduling, not by the removed work. Each
event still spawns one job task that the 4 worker threads, shared with the 4
drains and the producers, must schedule, poll and free; at 270 000-440 000
events/s that is about 9-15 µs of CPU per event across the 4 threads, while the
removed allocations, metric-key builds and locale `Arc` cost a few hundred
nanoseconds per handler. The gain (up to 1.09× with 4 handlers, where the saved
`Event` clones and `join_all` copies add up) stays within the host's
run-to-run spread (Story 16.2 saw the P = 1 push cost vary 145-294 ns/item
between runs). Reaching 1.3× would need fewer task spawns per event (running a
single concurrent handler inside the drain task, or batching jobs), which
changes the AD-10 worker contract and is out of this story's scope.

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
