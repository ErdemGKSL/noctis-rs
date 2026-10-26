# Cluster deployment

A Noctis App shards on its own: one process asks Discord for the recommended shard
count and runs every shard. When one process is no longer enough (memory, CPU, or
restarts that should not take the whole bot offline), run the bot as a **Cluster**:
several processes of the same binary, each running one **Shard Range** of a fixed
shard total. Handlers do not change; the deployment does.

This guide covers what every process must agree on and what must be shared. The
reference for every builder method and variable named here is the Usage Skill's
[app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md)
(§2 environment variables, §4 sharding and Clusters, §5 HTTP) and
[adapters.md](../../examples/project-template/.claude/skills/noctis/references/adapters.md)
(§6 namespacing, §7 Clusters).

Stay on one process while it copes: a Cluster adds a rate-limit proxy, shared
Adapters and an identify queue to operate. Discord recommends about one shard per
1000 guilds, so most bots never need one.

## Shard Ranges

Two variables (or builder methods) define a process's part of the Cluster:

| Variable | Builder | Meaning |
| --- | --- | --- |
| `NOCTIS_SHARD_TOTAL` | `.shard_total(u32)` | the fixed shard total, **the same on every process** |
| `NOCTIS_SHARD_RANGE` | `.shard_range(start..end)` | the shards this process runs, half-open: `32..64` is shards 32-63 |

`NOCTIS_SHARD_RANGE` is exactly `start..end` (`32..=63`, `32-63` or `32..` are
`EnvError::InvalidShardRange`). Shard ids stay global: shard 40 of 128 is shard 40 on
every process, in identify bucket `40 % max_concurrency`.

`build()` rejects a range that cannot work, with every problem listed at once in its
`BuildError`:

- `BuildViolation::EmptyShardRange { start, end }`: start is not below end;
- `BuildViolation::ShardRangeWithoutTotal { start, end }`: a range without a fixed
  total;
- `BuildViolation::ShardRangeOutOfBounds { start, end, total }`: the end is above the
  total;
- `BuildViolation::InvalidShardTotal`: a total of 0.

At `run()`, a total below ⌈recommended × 2 / 5⌉ is `StartError::ShardTotalTooSmall`
(a shard holds at most 2500 guilds, GW-03), and `StartError::SessionStartLimit` stops
a process whose remaining session starts cannot cover its own range (GW-02; the check
is per process).

**Align the ranges to `max_concurrency`** (from `GET /gateway/bot`; 1 for most bots,
16 or more for large ones): each range's start, and its end unless it equals the
total, should be a multiple of it. A misaligned range still runs, with one
`noctis::app` warning ("shard range is not aligned to max_concurrency; identify
buckets are split across processes"). Keep the ranges consecutive, so that together
they cover `0..total` exactly once.

A process logs `starting shards {start}..{end} of {total}`, and its readiness, its
`AllShardsReady` hook, its `noctis_shard_status` gauges and
`RunError::AllShardsEnded` cover only its own shards.

Gateway commands go to the shard that owns the guild. When that shard runs in another
process, `ctx.gateway().request_guild_members(..)` and voice state updates return
`GatewayError::GuildNotInRange { guild, shard, range }` and send nothing; ask the
owning process through your own queue. Interactions and events for a guild always
arrive at the process that owns its shard.

## IdentifyQueue

Discord allows one IDENTIFY per bucket (`shard_id % max_concurrency`) every 5 s for
the whole bot, not per process. Every shard of every process takes a lease from the
`IdentifyQueue` port before it identifies, so the Cluster needs **one shared
implementation** of it:

- `MemoryIdentifyQueue`, the default, serializes per bucket inside one process only.
  Two processes with their own memory queues can identify the same bucket at once.
- Across processes, write an Adapter over a shared store (Redis, a database) that
  grants at most one lease per bucket per 5 s across the whole Cluster, with different
  buckets in parallel, and pass it to `.identify_queue(..)` on every process.
- The queue must have at least `max_concurrency` buckets; a shard whose bucket is past
  the queue's size never identifies.
- It is never namespaced by App id, and `.identify_queue(Shared::new(..))` does not
  compile: the queue is keyed by bucket.
- Test the Adapter with the conformance suite
  `noctis::testkit::conformance::identify_queue` (feature `testkit`): it checks the
  lease bucket, 5 s spacing within a bucket and parallel buckets.

Several Apps inside **one** process share one queue by handing each builder the same
`Arc<MemoryIdentifyQueue>`:

```rust
use std::num::NonZeroU16;
use std::sync::Arc;

use noctis::ports::memory::MemoryIdentifyQueue;
use noctis::prelude::*;

let max_concurrency = NonZeroU16::new(16).expect("non-zero");
let queue = Arc::new(MemoryIdentifyQueue::new(max_concurrency));
let first = App::from_env()?
    .shard_total(128)
    .shard_range(0..64)
    .identify_queue(Arc::clone(&queue))
    .publisher(true)
    .build()?;
let second = App::from_env()?
    .shard_total(128)
    .shard_range(64..128)
    .identify_queue(queue)
    .build()?;
```

A resumed shard sends RESUME instead of IDENTIFY and takes no lease.

## HTTP proxy

Discord's global limit of 50 requests per second is per bot token, so every process of
the Cluster shares it. Point every process at **one shared rate-limit proxy**:

- `NOCTIS_HTTP_PROXY=http://proxy:3000` or `.http_proxy("proxy:3000")`. Accepted forms
  are `host`, `host:port`, `[ipv6]` and `[ipv6]:port`, optionally prefixed with
  `http://`; HTTPS proxies, paths and credentials fail `build()` with
  `BuildViolation::InvalidHttpProxy`.
- With a proxy, the App's local rate limiter is off: the proxy limits for everyone.
  Every Discord request goes through it: twilight calls, raw REST, webhooks and
  interaction callbacks. A 429 behind the proxy is never retried; it fails once as
  `RateLimited` (RL-01).
- The proxy composes with Noctis's Discord HTTP edge: requests still pass the App's
  loopback edge, which now forwards to `http://{proxy}/api/v10/..` over HTTP/1.1 (or
  h2c with `HttpConfig::upstream_h2c`) instead of to Discord.
- A process that runs a Shard Range smaller than the total without a proxy logs one
  `noctis::app` warning at `run()`: "this process runs a Shard Range of a Cluster
  without an HTTP proxy: … (RL-02)". It still runs, but the Cluster can exceed the
  global limit.

## Shared Adapters

Every process must reach the same data, so the ports that hold cross-process state
need Adapters over a shared store (Noctis 0.1 ships only in-memory defaults; write the
Adapter and run its `noctis::testkit::conformance` suite):

| Port | In a Cluster |
| --- | --- |
| `RefStore` | shared: a Ref minted by one process is clicked on another |
| `RateLimitStore` | shared: a declared `rate_limit(..)` counts across processes |
| `ModuleStateStore` | shared: a Module toggled on one process applies on every process at its next poll (`.module_state_poll(..)`, 5 s by default) |
| `IdentifyQueue` | shared (see [IdentifyQueue](#identifyqueue)) |
| `CacheStore` (`ctx.kv()`), `RenderCache`, `AssetStore` | usually shared, so every process sees the same data and renders |
| `SessionStore` | persistent, so a restarted process resumes its shards; each process reads and writes only its own shards' entries |
| `DiscordCache` | per process: it holds what this process's shards receive |
| `LocaleSource`, `GuildAccessList` | per process (same configuration everywhere) |

**Give every process the same App id.** Core prefixes every byte-store key with
`<app_id>:<port>:` through `noctis::ports::Namespaced` (`<app>:ref:`, `<app>:session:`,
`<app>:cache:`, …), so processes with different ids would not see each other's data.
Set `NOCTIS_APP_ID` (or `.app_id(..)`) to the same `[a-z0-9-]{1,32}` value
everywhere, or rely on the default (the bot user id from the token), which is the
same for every process of one bot. Several bots sharing one store stay apart because
their ids differ; opt one port into cross-App sharing with `Shared::new(..)`.

Session entries live under `<app>:session:shard:<id>:<total>`, so a process resumes
only its own shards and a new shard total never resumes an old layout. Session resume
needs a `SessionStore` that outlives the process; the memory default resumes only
within one process.

## NOCTIS_PAYLOAD_KEY

A `signed` Route's `custom_id` is signed by the process that sent the message and
verified by whichever process receives the click. Set **the same**
`NOCTIS_PAYLOAD_KEY` (64 hex digits; `openssl rand -hex 32`) on every process, or a
click handled by another process fails as `forged`.

To rotate it, deploy the new key as `NOCTIS_PAYLOAD_KEY` and the old one as
`NOCTIS_PAYLOAD_KEY_PREVIOUS` on every process: ids signed with either key verify,
and new ids use the new key. Remove the previous key once the components signed with
it no longer matter. A previous key without a current one fails `build()` with
`BuildViolation::PreviousKeyWithoutKey`.

Noctis never logs the key (`Debug` shows only that one is set), and an invalid value
is `EnvError::InvalidPayloadKey`, whose message names the variable, never the value.
Keep it in your secret store, never in a file in the repository.

## One publisher

Publish never runs on READY. In a Cluster, exactly one process publishes:

- set `NOCTIS_PUBLISHER=true` (or `.publisher(true)`) on one process and leave it unset
  or `false` on the others. That process publishes globally once at startup, before it
  connects its shards; a failed Publish is one `error` log and the process keeps
  starting. The others send no Publish request;
- or run `--publish` once from the deploy step (`./bot --publish`) and set no publisher
  at all.

Two publishers race each other's bulk overwrites and waste Discord's daily create
quota (REG-05).

## Voice

A guild's voice call lives in the process whose Shard Range owns the guild's shard:
its Voice State Update goes out on that shard and Discord answers on it. A `/play`
interaction for the guild arrives at the same process, so the usual handler works
unchanged. `join` in any other process is `VoiceError::Gateway(GuildNotInRange)`, and
each process leaves its own calls at shutdown. The
[voice guide's Cluster notes](voice.md#7-cluster-notes) have the details.

## Example

A bot with 128 shards on four processes, `max_concurrency` 16:

| Process | `NOCTIS_SHARD_RANGE` | `NOCTIS_PUBLISHER` |
| --- | --- | --- |
| 0 | `0..32` | `true` |
| 1 | `32..64` | — |
| 2 | `64..96` | — |
| 3 | `96..128` | — |

Every process also gets the same:

```text
NOCTIS_TOKEN=<bot token, from the secret store>
NOCTIS_SHARD_TOTAL=128
NOCTIS_HTTP_PROXY=http://discord-proxy:3000
NOCTIS_PAYLOAD_KEY=<64 hex digits, from the secret store>
NOCTIS_APP_ID=my-bot
```

The binary builds its Adapters (shared `RefStore`, `IdentifyQueue`, and so on) in its
`configure` function, so every process runs the same code and differs only in its
environment. The memory estimate of the skill's §4 ("Sizing memory") applies per
process, to the guilds of its own range.

## Checklist

1. The same `NOCTIS_SHARD_TOTAL` on every process; ranges consecutive, covering
   `0..total` once, aligned to `max_concurrency`.
2. One shared `IdentifyQueue` Adapter on every process (`.identify_queue(..)`).
3. One shared HTTP rate-limit proxy (`NOCTIS_HTTP_PROXY`) on every process.
4. Shared `RefStore`, `RateLimitStore`, `ModuleStateStore` (and usually `CacheStore`,
   `RenderCache`, `AssetStore`); a persistent `SessionStore`; the same App id.
5. The same `NOCTIS_PAYLOAD_KEY` (and, during a rotation,
   `NOCTIS_PAYLOAD_KEY_PREVIOUS`) on every process.
6. Exactly one publisher (`NOCTIS_PUBLISHER=true` on one process), or `--publish` in
   the deploy step.
7. Keep `NOCTIS_SHUTDOWN_GRACE` plus 7 s (plus up to 5 s of Shutdown Tasks, such as
   the voice leave) below your orchestrator's kill timeout, so each process closes its
   shards resumably.
