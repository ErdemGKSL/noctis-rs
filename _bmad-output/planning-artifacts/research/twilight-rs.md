# twilight-rs ecosystem: state of the art for noctis (research, 2026-10-19)

Scope: everything noctis (a tokio-based, sharded Discord bot framework for bots in millions of guilds) needs to know about the twilight crates as they stand on 2026-10-19.

Sources: crates.io API (queried 2026-10-19), the published crate sources in `~/.cargo/registry`, `twilight-rs/twilight` `main` (HEAD `6a540ec`, 2026-10-18) including `examples/`, per-crate `CHANGELOG.md` files and `book/`, and `twilight-rs/http-proxy` `main`. Every code sketch marked **[compiled]** was built with `cargo build` (rustc 1.97, edition 2024) against the exact versions listed in §0, and clippy reported nothing beyond style lints. Nothing was run against the live Discord API.

> **Naming note.** The project was first called noctis, but the crates.io name `noctis` is taken by AlvaroMS25/noctis, a twilight slash-command framework. The project is now noctis; `noctis` and `noctis-*` were free on crates.io on 2026-10-19. The established twilight derive crate is `twilight-interactions` (0.17.1, 2026-05-27, about 58k downloads), so expect comparisons with it.

---

## 0. Versions table (crates.io, 2026-10-19)

| Crate | Latest stable | Released | MSRV | Notes |
|---|---|---|---|---|
| twilight-gateway | **0.17.1** | 2025-12-13 | 1.89 | features: `zstd` (default), `zlib`, `simd-json`, TLS (`rustls-platform-verifier` default, `rustls-native-roots`, `rustls-webpki-roots`, `native-tls`), `twilight-http` (default; deprecated on main) |
| twilight-http | **0.17.1** | 2025-12-13 | 1.89 | features: `decompression` (default, brotli), `simd-json`, `hickory`, TLS features as above |
| twilight-model | **0.17.1** | 2025-12-13 | 1.89 | Components V2 + modal Label/FileUpload |
| twilight-cache-inmemory | **0.17.1** | 2025-12-13 | 1.89 | feature `permission-calculator` |
| twilight-http-ratelimiting | **0.17.1** | 2025-12-13 | 1.89 | **0.17.0 can panic: pin ≥ 0.17.1** |
| twilight-standby | 0.17.0 | 2025-11-08 | 1.89 | |
| twilight-util | 0.17.0 | 2025-11-08 | 1.89 | features `builder`, `link`, `permission-calculator`, `snowflake`, `full` |
| twilight-mention | 0.17.0 | 2025-11-08 | 1.89 | |
| twilight-validate | 0.17.0 | 2025-11-08 | 1.89 | |
| twilight-gateway-queue | 0.17.0 | 2025-11-08 | 1.89 | |
| twilight-lavalink | **0.16.0-rc.1** (latest stable 0.15.3) | 2024-02-21 | – | Speaks the Lavalink **v3** API. A v4 rewrite (#2322) is on `main` but unreleased. |
| twilight-http-proxy | not on crates.io | `main` 2026-05-19 | – | Server binary (Docker), depends on `twilight-http-ratelimiting = "0.17"` |
| nirn-proxy (Go) | v1.3.3 tag | – | – | Alternative REST proxy |
| twilight-interactions (3rd party) | 0.17.1 | 2026-05-27 | – | derive macros for commands and modals |
| songbird (voice) | 0.6.1 | 2026-10-10 | – | supports twilight |
| lavalink-rs | 0.15.0 | 2026-03-01 | – | Lavalink v4 client with twilight support |

**Unreleased `main` (76 commits since 0.17.1, as of 2026-10-18).** The next release will probably be 0.18, because `main` already carries breaking (`!`) commits:
- `feat(gateway)!: add channel_type to message create event` (#2533). Breaking for anyone who constructs `MessageCreate`.
- `fix(http)!: serialize create role icon as string` (#2552).
- `fix(http, model)!: Interaction Callback` (#2493). `create_response(..).with_response()` now returns `InteractionCallbackResponse` instead of `InteractionResponse`.
- `feat(lavalink)!: Lavalink v4 refactor` (#2322).
- New `twilight_gateway::bucket(bucket_id, buckets, shards) -> impl ExactSizeIterator<Item = ShardId>` (#2511). `create_bucket` and `create_recommended` are deprecated "since 0.17.2". The `twilight-http` feature of the gateway is deprecated (#2538).
- Checkbox / CheckboxGroup components (types 23 and 22, #2516), `InteractionResponse` builders in util (#2506), `Interaction.attachment_size_limit`, monetization subscriptions, embed flags, and more.
- Fix #2592: `UnfurledMediaItem.content_type` is no longer serialized as `null`. **0.17.1 still sends `"content_type": null`**, which I verified by running the interactions sketch.

---

## 1. Gateway architecture

### 1.1 Building shards (0.17.1 API)

- `Config::new(token, intents)` builds a config directly. `ConfigBuilder::new(token, intents)` exposes `.queue(Q)`, `.session(Session)`, `.resume_url(String)`, `.presence(..)`, `.identify_properties(..)`, `.large_threshold(50..=250)`, `.proxy_url(..)` and `.ratelimit_messages(bool)`. `ConfigBuilder::from(Config)` lets you derive a per-shard config.
- `Shard::with_config(ShardId::new(n, total), config)`. `Shard<Q = InMemoryQueue>` is generic over the queue.
- Helpers in 0.17.1: `create_iterator(numbers, total, config, |id, builder| builder.build())`, `create_bucket(bucket_id, concurrency, total, ..)` and `async create_recommended(&http, config, ..)`. `create_recommended` calls `GET /gateway/bot` and creates `0..shards`.
  - On `main`, `create_bucket` is deprecated because it **creates non-consecutive shards** (it uses `step_by(concurrency)`). `create_recommended` is deprecated in favor of calling `http.gateway().authed()` yourself.
  - The replacement is `bucket(id, buckets, shards)`, which yields consecutive `ShardId` ranges. **Recommendation:** noctis should compute ranges itself: `ShardId::new(n, total)` over a consecutive range, plus `Shard::with_config`. That code works on both 0.17 and 0.18.
- `Config` is `Clone`, and `InMemoryQueue` is a cheap `Arc`-like handle, so cloning the base config shares one identify queue across all shards.

### 1.2 Identify queue (`max_concurrency`)

- `trait Queue { fn enqueue(&self, id: u32) -> oneshot::Receiver<()> }`. It is object safe and implemented for `&T`. Sending `()` releases the shard. If the sender is dropped, the shard re-enqueues.
- `InMemoryQueue::new(max_concurrency, remaining, reset_after, total)` processes buckets keyed by `shard_id % max_concurrency` (one identify per bucket every 5 s) and respects the daily `remaining`/`total`. `InMemoryQueue::update(..)` changes the settings at runtime.
- **Gotcha:** `InMemoryQueue::default()` is `max_concurrency = 1, remaining = 1000, total = 1000`. A bot with 2,000+ shards boots roughly 16× slower than it needs to if you forget to feed it the `session_start_limit` from `GET /gateway/bot`.
- **Gotcha:** `InMemoryQueue::new` (like `RateLimiter::new`) calls `tokio::spawn`, so it **must be called inside a runtime**.
- Multi-process: implement `Queue` over a shared service. Upstream has an HTTP-queue example (`examples/gateway-queue-http.rs`), and redis also works. The queue must serialize identifies **per bucket across all processes**, because one token has one `max_concurrency`.

### 1.3 One task per shard vs one merged stream

- `Shard` implements `futures::Stream<Item = Result<Message, ReceiveMessageError>>` (since 0.16). `StreamExt::next_event(EventTypeFlags)` is cancel-safe and returns `Option<Result<Event, ReceiveMessageError>>`. `None` means the shard is fatally closed (bad token, invalid intents, and so on).
- Upstream examples (`gateway-reshard.rs`) use **one tokio task per shard** in a `JoinSet`. That is the right model for a multi-threaded runtime: shards spread across worker threads, and each task owns its `Shard` (`&mut` needed). The alternative, `futures::stream::select_all` over many shards on one task, serializes JSON parsing on one thread. Avoid it at scale.
- Errors from `next_event` are **non-fatal**: log them and `continue`. Reconnects and resumes happen automatically inside the shard. Error kinds: `Compression`, `Deserializing { event: String }` (unknown or invalid payloads) and `Reconnect`.
- Sending gateway commands (presence, request members, voice state) from other tasks: `shard.sender()` returns `MessageSender` (Clone; `.command(&impl Command)`, `.send(String)`, `.close(CloseFrame)`). Store one per shard in a `Vec<MessageSender>` indexed by shard id. Outgoing commands are rate limited by the shard's `CommandRatelimiter` (120 per 60 s, with heartbeat slots reserved).

### 1.4 Session resume and graceful shutdown

- `shard.close(CloseFrame::RESUME)` (code 4000) keeps the session resumable. `CloseFrame::NORMAL` (1000) or 1001 **invalidates** the session and drops `session` and `resume_url`. After `close`, keep polling until `Event::GatewayClose(_)` (or `Message::Close`) arrives.
- `Session` is `Serialize + Deserialize + Clone` (`Session::new(sequence, id)`; `.id()`, `.sequence()`). Persist `shard.session()` and `shard.resume_url()` (redis or a file) on shutdown. On restart, pass them via `ConfigBuilder::session(..).resume_url(..)` and the shard sends RESUME instead of IDENTIFY. That avoids identify-quota use and guild-create floods during rolling deploys. Discord only keeps sessions for a short while, so this is useful for fast restarts, not for long outages.
- 0.17 gateway fixes that matter: zombied connections are terminated (#2435), the connect timeout is 10 s (#2448), close frames are no longer swallowed (#2399, 0.16), and `main` adds a delay between repeated connect attempts (#2505).

### 1.5 Resharding

Upstream pattern (`examples/gateway-reshard.rs`):
1. Periodically (for example every 8 h) call `GET /gateway/bot`. If `shards` changed, build a new shard set with the new total.
2. Start the new shards **alongside** the old ones and poll them until at least 3/4 are identified (`shard.state().is_identified()`), bounded by an estimate built from `max_concurrency` and `remaining`/`reset_after`.
3. Swap the sets: drop the old `JoinSet`, which aborts its tasks. During overlap you get duplicate events, so handlers must be idempotent or deduplicate by guild ownership.

The identify budget is the binding constraint: each reshard re-identifies every shard. Large bots should pick a shard count with headroom (Discord requires a multiple of `max_concurrency` (16) for large-bot sharding) and reshard rarely.

### 1.6 Multi-process / cluster layout

- Each process owns a consecutive range `[cluster_id * per, min((cluster_id + 1) * per, total))` of `ShardId::new(n, total)`. Keep ranges aligned to multiples of `max_concurrency` so every process can identify a full bucket in parallel.
- What must be shared across processes: (a) the identify queue, (b) the **HTTP global rate limit** (50 req/s per token by default, *not* per process). Use twilight-http-proxy or nirn-proxy and set `ClientBuilder::proxy(url, true).ratelimiter(None)`. (c) Optionally the cache (external) and sessions (for resume).

### 1.7 Event filtering, intents, compression

- **Intents** decide what Discord sends. **`EventTypeFlags`** decide what twilight *deserializes*. `twilight_gateway::parse` first runs `GatewayEventDeserializer::from_json`, which reads only `op`/`t`. If the event type is not wanted, the `String` is dropped without a serde pass. That is the single biggest CPU lever: pass the narrowest flags to `next_event`. Flags are `u128` bitflags, `const`-composable via `.union()`, and include intent-shaped groups (`EventTypeFlags::GUILDS`, `GUILD_MESSAGES`, and so on). An "Intents → EventTypeFlags" helper was added and then reverted on `main` (#2531 → #2539), so noctis should own that mapping.
- Close messages always surface as `Event::GatewayClose`. 0.17.1 adds `Event::RateLimited` / `EventTypeFlags::RATE_LIMITED`, the gateway "you are being rate limited" opcode for request-guild-members.
- Privileged intents (`GUILD_MEMBERS`, `GUILD_PRESENCES`, `MESSAGE_CONTENT`) need approval for verified bots and **dominate bandwidth, CPU and cache memory**. An interaction-first framework should default to `Intents::GUILDS` (plus `GUILD_VOICE_STATES` if voice) and make everything else opt-in.
- Compression: `zstd` (default since 0.17, `compress=zstd-stream`) or `zlib` (`zlib-stream`, un-deprecated in 0.17 and now backed by flate2/zlib-rs). They are mutually exclusive, with zstd preferred when both are on. Disabling both sends uncompressed JSON. zstd saves bandwidth with lower CPU cost per byte than zlib. Keep the default.
- `large_threshold` (default 50, max 250) controls how many offline members are sent in GUILD_CREATE when you have `GUILD_MEMBERS`. Keep it low for big bots.

### 1.8 Sketch: per-shard tasks + bounded worker pool + resumable shutdown **[compiled]**

`Cargo.toml` used for all sketches:

```toml
[dependencies]
anyhow = "1"
rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }
serde_json = "1"
tokio = { version = "1.53", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"
twilight-cache-inmemory = "0.17.1"
twilight-gateway = { version = "0.17.1", features = ["zstd", "simd-json"] }
twilight-gateway-queue = "0.17"
twilight-http = { version = "0.17.1", features = ["simd-json"] }
twilight-http-ratelimiting = "0.17.1"
twilight-mention = "0.17"
twilight-model = "0.17.1"
twilight-standby = "0.17"
twilight-util = { version = "0.17", features = ["builder", "permission-calculator", "snowflake"] }
twilight-validate = "0.17"
```

```rust
//! Sketch 1: multi-shard gateway runner (twilight 0.17.1).
//! One tokio task per shard -> bounded mpsc -> fixed worker pool.
use std::{collections::HashMap, env, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util_shim::CancellationToken;
use twilight_gateway::{
    queue::InMemoryQueue, CloseFrame, ConfigBuilder, Event, EventTypeFlags, Intents, Session,
    Shard, ShardId, StreamExt as _,
};
use twilight_http::Client;

/// Minimal stand-in so the sketch has no extra deps; use tokio_util's in noctis.
mod tokio_util_shim {
    pub use tokio::sync::watch;
    #[derive(Clone)]
    pub struct CancellationToken(watch::Sender<bool>, watch::Receiver<bool>);
    impl CancellationToken {
        pub fn new() -> Self { let (tx, rx) = watch::channel(false); Self(tx, rx) }
        pub fn cancel(&self) { let _ = self.0.send(true); }
        pub async fn cancelled(&self) { let mut rx = self.1.clone(); let _ = rx.wait_for(|v| *v).await; }
    }
}

/// Events the framework actually handles. Everything else is skipped *before*
/// deserialization (parse() only reads op + t, then drops the String).
const WANTED: EventTypeFlags = EventTypeFlags::READY
    .union(EventTypeFlags::RESUMED)
    .union(EventTypeFlags::INTERACTION_CREATE)
    .union(EventTypeFlags::GUILD_CREATE)
    .union(EventTypeFlags::GUILD_DELETE)
    .union(EventTypeFlags::RATE_LIMITED);

struct Dispatch {
    shard: ShardId,
    event: Event,
}

/// Cluster slice of the global shard space (multi-process deployments).
struct ClusterSpec {
    cluster_id: u32,
    clusters: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // twilight 0.17 uses rustls 0.23 without picking a provider: install one first.
    rustls::crypto::ring::default_provider().install_default().expect("crypto provider");
    tracing_subscriber::fmt::init();
    let token = env::var("DISCORD_TOKEN")?;
    // RateLimiter::new spawns a tokio task: build the client inside the runtime.
    let http = Arc::new(Client::new(token.clone()));

    // 1. Ask Discord for shard count + identify limits.
    let info = http.gateway().authed().await?.model().await?;
    let total = info.shards; // or a fixed, pre-planned count (multiple of max_concurrency)
    let ssl = &info.session_start_limit;

    // 2. Identify queue honoring max_concurrency. Default InMemoryQueue is
    //    max_concurrency=1 -> very slow boot for big bots. For multi-process
    //    clusters, replace with a shared queue (HTTP/redis) implementing `Queue`.
    let queue = InMemoryQueue::new(
        ssl.max_concurrency,
        ssl.remaining,
        Duration::from_millis(ssl.reset_after),
        ssl.total,
    );

    let config = ConfigBuilder::new(token, Intents::GUILDS)
        .queue(queue)
        .build();

    // 3. Shards owned by this process. Keep consecutive ranges aligned to
    //    max_concurrency so buckets (shard_id % max_concurrency) fill evenly.
    let spec = ClusterSpec { cluster_id: 0, clusters: 1 };
    let per = total.div_ceil(spec.clusters);
    let range = (spec.cluster_id * per)..((spec.cluster_id + 1) * per).min(total);

    // Sessions persisted from a previous process (e.g. redis) => resume, not re-identify.
    let saved: HashMap<u32, (Session, String)> = HashMap::new();

    let shards: Vec<Shard<InMemoryQueue>> = range
        .map(|n| {
            let id = ShardId::new(n, total);
            let mut b = ConfigBuilder::from(config.clone());
            if let Some((session, resume_url)) = saved.get(&n).cloned() {
                b = b.session(session).resume_url(resume_url);
            }
            Shard::with_config(id, b.build())
        })
        .collect();

    // 4. Bounded channel = backpressure. Workers do the async handling.
    let (tx, rx) = mpsc::channel::<Dispatch>(8192);
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut workers = JoinSet::new();
    for _ in 0..num_workers() {
        let rx = rx.clone();
        let http = http.clone();
        workers.spawn(async move {
            loop {
                let Some(d) = rx.lock().await.recv().await else { break };
                handle(&http, d).await;
            }
        });
    }

    let ct = CancellationToken::new();
    let mut shard_tasks = JoinSet::new();
    for shard in shards {
        shard_tasks.spawn(runner(shard, tx.clone(), ct.clone()));
    }
    drop(tx);

    tokio::signal::ctrl_c().await?;
    ct.cancel();
    // Collect sessions for persistence -> next process resumes them.
    while let Some(res) = shard_tasks.join_next().await {
        if let Ok((id, Some(session), resume_url)) = res {
            tracing::info!(shard = %id, seq = session.sequence(), ?resume_url, "saved session");
        }
    }
    while workers.join_next().await.is_some() {}
    Ok(())
}

fn num_workers() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get() * 4)
}

#[tracing::instrument(skip_all, fields(shard = %shard.id()))]
async fn runner(
    mut shard: Shard<InMemoryQueue>,
    tx: mpsc::Sender<Dispatch>,
    ct: CancellationToken,
) -> (ShardId, Option<Session>, Option<String>) {
    let mut closing = false;
    loop {
        let item = tokio::select! {
            biased;
            _ = ct.cancelled(), if !closing => {
                // RESUME (4000) keeps the session valid; NORMAL (1000) invalidates it.
                shard.close(CloseFrame::RESUME);
                closing = true;
                continue;
            }
            item = shard.next_event(WANTED) => item,
        };
        let Some(item) = item else { break };
        let event = match item {
            Ok(Event::GatewayClose(_)) if closing => break,
            Ok(event) => event,
            Err(source) => {
                tracing::warn!(?source, "error receiving event");
                continue;
            }
        };
        // Backpressure: awaiting here stops reading the socket for THIS shard only.
        // Discord will eventually disconnect a shard that doesn't read; a resume follows.
        if tx.send(Dispatch { shard: shard.id(), event }).await.is_err() {
            break;
        }
    }
    (
        shard.id(),
        shard.session().cloned(),
        shard.resume_url().map(ToOwned::to_owned),
    )
}

async fn handle(_http: &Client, d: Dispatch) {
    match d.event {
        Event::Ready(r) => tracing::info!(shard = %d.shard, guilds = r.guilds.len(), "ready"),
        Event::InteractionCreate(i) => tracing::debug!(id = %i.id, "interaction"),
        Event::RateLimited(r) => tracing::warn!(?r, "gateway ratelimited"),
        _ => {}
    }
}
```

Notes on the sketch:
- The `tokio_util_shim` only exists to avoid one dependency. Use `tokio_util::sync::CancellationToken` in noctis.
- `Arc<Mutex<Receiver>>` is the simplest MPMC worker pool. For production, prefer `async-channel` / `flume` (lock-free MPMC) or **per-shard-hash worker channels**: send to `workers[guild_id % N]`, which also preserves per-guild ordering.

---

## 2. HTTP client

- `Client::new(token)` or `Client::builder()` with `.token(..)`, `.timeout(Duration)` (default **10 s**), `.proxy(url, use_http)`, `.ratelimiter(Option<RateLimiter>)`, `.default_allowed_mentions(..)`, `.default_headers(..)` and `.remember_invalid_token(bool)` (default true: after a 401 every later request fails fast).
- `Client` is `Send + Sync`. Create **one** per process and share it as `Arc<Client>`. Requests are `IntoFuture`, so you `.await` them directly and then call `.model().await` to deserialize (or `.bytes()`/`.text()`).
- **0.17 ratelimiter rewrite (#2418):** `twilight_http_ratelimiting::RateLimiter` is now a concrete actor (one spawned tokio task per limiter, unbounded mpsc), *not* a trait. `RateLimiter::new(global_limit)`, `Default` = 50/s. `acquire(Endpoint) -> PermitFuture`, `permit.complete(Option<RateLimitHeaders>)`, `bucket(endpoint)`. It supports unknown paths dynamically (#2453). **Interaction and webhook endpoints bypass the global limit** (verified in `actor.rs`: `is_interaction()`).
- **429 handling (#2424):** `ResponseFuture` automatically and immediately retries rate-limited requests "until they succeed or fail with another error". The old retry-delay logic was removed (#2455). Without a local ratelimiter you **must** have a proxy that absorbs the retries. `ResponseFuture::set_pre_flight` cancels a request once it clears the queue (useful for spammy edits).
- Errors: `twilight_http::Error` with `ErrorType::{Response { status, error, .. }, RequestTimedOut, Unauthorized, Parsing, RequestCanceled, ..}`.
- **Gotcha (verified by running): `Client::new` panics** (a) outside a tokio runtime, because the ratelimiter calls `tokio::spawn`, and (b) if no rustls `CryptoProvider` is installed: "Could not automatically determine the process-level CryptoProvider". twilight's examples call `rustls::crypto::ring::default_provider().install_default()` first. noctis should do this in its bootstrap (or enable `aws-lc-rs`/`ring` on rustls so the provider is chosen automatically, or use `native-tls`).
- **Proxy:** `twilight-rs/http-proxy` (Rust, Docker image, uses `twilight-http-ratelimiting 0.17`, active 2026) or `nirn-proxy` (Go, supports clustering). Set `.proxy("proxy:3000".into(), true)` (`true` means plain HTTP to the proxy) and `.ratelimiter(None)`. This is mandatory once more than one process shares a token, because the global and per-route limits are per token.
- **Raw requests and escape hatch:** `RequestBuilder::raw(Method, "channels/123/messages".into()).json(&body).build()?` then `client.request::<T>(req)`. These go through the same ratelimiter and retries. Many message-creating builders also accept `.payload_json(&[u8])` (for example `create_followup`), which sends raw JSON through a typed endpoint.
- **Interaction client:** `http.interaction(application_id)` returns `InteractionClient` with `create_response(id, &token, &InteractionResponse)` (plus `.with_response()`), `response(token)`, `update_response(token)`, `delete_response(token)`, `create_followup(token)`, `followup(token, msg_id)`, `update_followup(..)`, `delete_followup(..)`, `set_global_commands(&[Command])` (bulk PUT), `set_guild_commands(guild, &[Command])`, `create_*`/`update_*`/`delete_*` single command, and `command_permissions(..)`. Fetch the application id once at startup (`http.current_user_application()`) and cache it.

## 3. Cache

- `InMemoryCache<CacheModels = DefaultCacheModels>` (`DefaultInMemoryCache`). Since 0.16 it is **generic over model types** via `trait CacheableModels { type Guild: CacheableGuild; type Member; type Channel; type Role; type User; type Message; ... }`, so you can store slim custom structs (`examples/cache-optimization/`). Storage is `DashMap`-based, `Send + Sync`, and is fed with `cache.update(&event)`.
- Config: `InMemoryCache::builder().resource_types(ResourceType::..).message_cache_size(n).build()`. `ResourceType` flags: `CHANNEL, EMOJI, GUILD, MEMBER, MESSAGE, PRESENCE, REACTION, ROLE, USER_CURRENT, USER, VOICE_STATE, STAGE_INSTANCE, INTEGRATION, STICKER, GUILD_SCHEDULED_EVENT`. `cache.stats()` gives counts. `cache.permissions()` (feature `permission-calculator`) computes guild and channel permissions.
- **Getters return `Reference<'_, K, V>` (a DashMap guard).** Never hold one across `.await` (deadlock or shard stall). Copy out what you need.
- Memory (measured `size_of` on 0.17.1, x86_64; heap strings, vecs and DashMap overhead come on top): `CachedGuild` 440 B, `CachedMember` 176 B, `User` 296 B, `Role` 160 B, `Channel` 1224 B, `Message` 3520 B, `Interaction` 5760 B. `Event` is 312 B because large payloads are boxed. Rough estimate for millions of guilds with the default models and members cached: several KB per guild before members. Members dominate (millions × (176 B + user + map overhead)). With `GUILD | CHANNEL | ROLE` only and custom slim models, a few hundred bytes to ~2 KB per guild is realistic. For 5 M guilds that is about 1-10 GB across the fleet, split per process by shard range.
- **When to go external (redis / keydb / dragonfly):** when several processes or services need the same view (dashboards, workers, a REST API), when restarts must not lose state (avoids GUILD_CREATE storms needing a cold cache), or when per-process RAM is the bottleneck. The cost is network latency on every read, serialization CPU and eventual consistency. A common design: gateway processes write deltas to redis (pipeline, compact encoding), and handlers read through a small local LRU.
- **Pluggable design for noctis** (sketch below): a sync `CacheBackend: Send + Sync` trait (`update(&Event)`, typed getters returning owned or `Arc` values), with implementations `InMemory` (wrapping twilight with slim `CacheableModels`), `NoCache`, and `External` (async getters behind a separate `AsyncCache` trait or a `CacheReader` that returns futures). Let users choose `ResourceType` and intents together, and have noctis refuse invalid combinations (for example a member cache without `GUILD_MEMBERS`).

### 3.1 Sketch: HTTP config, raw request, pluggable cache **[compiled, run]**

```rust
//! Sketch 3: HTTP client config, raw requests, pluggable cache (twilight 0.17.1).
use std::{sync::Arc, time::Duration};
use twilight_cache_inmemory::{DefaultInMemoryCache, InMemoryCache, ResourceType};
use twilight_gateway::Event;
use twilight_http::{
    client::ClientBuilder,
    request::{Method, Request},
    Client,
};
use twilight_http_ratelimiting::RateLimiter;
use twilight_model::{
    guild::Permissions,
    id::{marker::{ChannelMarker, GuildMarker, UserMarker}, Id},
};

/// Shared HTTP client. `Client` is Send + Sync; wrap in Arc once.
pub fn http(token: String, proxy: Option<String>) -> Arc<Client> {
    let mut b: ClientBuilder = Client::builder().token(token).timeout(Duration::from_secs(15));
    b = match proxy {
        // twilight-http-proxy / nirn-proxy owns ratelimiting -> disable local limiter.
        Some(url) => b.proxy(url, true).ratelimiter(None),
        // Default global limit is 50 req/s; raise only if Discord granted more.
        None => b.ratelimiter(Some(RateLimiter::new(50))),
    };
    Arc::new(b.build())
}

/// Escape hatch for endpoints/fields twilight doesn't model yet.
pub async fn raw_v2_message(http: &Client, channel: Id<ChannelMarker>) -> anyhow::Result<serde_json::Value> {
    let body = serde_json::json!({
        "flags": 1 << 15, // IS_COMPONENTS_V2
        "components": [{ "type": 17, "components": [{ "type": 10, "content": "raw JSON" }] }]
    });
    let req = Request::builder_raw(Method::Post, format!("channels/{channel}/messages"), &body)?;
    // Goes through the same ratelimiter + 429 retry as typed requests.
    let resp = http.request::<serde_json::Value>(req).await?;
    Ok(resp.model().await?)
}

trait RequestExt {
    fn builder_raw(m: Method, path: String, body: &serde_json::Value) -> Result<Request, twilight_http::Error>;
}
impl RequestExt for Request {
    fn builder_raw(m: Method, path: String, body: &serde_json::Value) -> Result<Request, twilight_http::Error> {
        twilight_http::request::RequestBuilder::raw(m, path).json(body).build()
    }
}

/// Cache tuned for a huge interaction-first bot: no members/presences/messages.
pub fn lean_cache() -> DefaultInMemoryCache {
    InMemoryCache::builder()
        .resource_types(
            ResourceType::GUILD | ResourceType::CHANNEL | ResourceType::ROLE | ResourceType::USER_CURRENT,
        )
        .message_cache_size(0)
        .build()
}

/// Framework-level cache abstraction: sync, cheap, object safe.
/// Backends: in-memory (twilight), none, or external (redis via a separate
/// gateway->cache worker; reads go async through a different trait).
pub trait CacheBackend: Send + Sync + 'static {
    fn update(&self, event: &Event);
    fn guild_name(&self, id: Id<GuildMarker>) -> Option<String>;
    fn member_permissions(&self, _guild: Id<GuildMarker>, _user: Id<UserMarker>) -> Option<Permissions> {
        None
    }
}

impl CacheBackend for DefaultInMemoryCache {
    fn update(&self, event: &Event) {
        InMemoryCache::update(self, event);
    }
    fn guild_name(&self, id: Id<GuildMarker>) -> Option<String> {
        // Never hold a cache Reference across .await - it is a DashMap guard.
        self.guild(id).map(|g| g.name().to_owned())
    }
}

pub struct NoCache;
impl CacheBackend for NoCache {
    fn update(&self, _: &Event) {}
    fn guild_name(&self, _: Id<GuildMarker>) -> Option<String> {
        None
    }
}

#[tokio::main]
async fn main() {
    // twilight 0.17 uses rustls 0.23 without picking a provider: install one first.
    rustls::crypto::ring::default_provider().install_default().expect("crypto provider");
    let _h = http("x".into(), None);
    let cache: Arc<dyn CacheBackend> = Arc::new(lean_cache());
    let _ = cache.guild_name(Id::new(1));
    println!("ok");
}
```

---

## 4. Interactions

### 4.1 Application command model (`twilight_model::application::command`)

- `Command { application_id, contexts: Option<Vec<InteractionContextType>>, default_member_permissions: Option<Permissions>, dm_permission (deprecated → contexts), description, description_localizations, guild_id, id, integration_types: Option<Vec<ApplicationIntegrationType>>, kind: CommandType, name, name_localizations, nsfw, options: Vec<CommandOption>, version }`.
- `CommandType::{ChatInput, User, Message, Unknown(u8)}`. **Primary Entry Point (type 4, Activities) is not modeled** and arrives as `Unknown(4)`. Likewise, `InteractionResponseType::LaunchActivity` (12) is missing. `CommandOption` uses `kind: CommandOptionType` (SubCommand, SubCommandGroup, String, Integer, Boolean, User, Channel, Role, Mentionable, Number, Attachment), `autocomplete`, `choices`, `channel_types`, `min/max_value`, `min/max_length` and localizations. 0.17 added associated types and `From` impls on `CommandOption(Value)` (#2415, #2413).
- `InteractionContextType::{Guild, BotDm, PrivateChannel}` and `ApplicationIntegrationType::{GuildInstall, UserInstall}` (in `twilight_model::oauth`) handle user-installable apps.
- Builders: `twilight_util::builder::command::{CommandBuilder, SubCommandGroupBuilder, SubCommandBuilder, StringBuilder, IntegerBuilder, NumberBuilder, BooleanBuilder, UserBuilder, ChannelBuilder, RoleBuilder, MentionableBuilder, AttachmentBuilder}`. `.validate()` runs `twilight-validate` (name/description lengths, option order, the 8000-char total, which counts chars since #2555).
- Registration: `set_global_commands(&cmds)` / `set_guild_commands(guild, &cmds)` are bulk-overwrite PUTs. They are idempotent, so call them from a deploy step or once per process start behind a hash check, **never per shard READY**. Global commands are rate limited (200 creates per day per application).

### 4.2 Receiving and responding

- `Event::InteractionCreate(Box<InteractionCreate>)` derefs to `Interaction { id, application_id, kind: InteractionType, data: Option<InteractionData>, token, guild_id, channel, member, user, locale, guild_locale, app_permissions, entitlements, context, authorizing_integration_owners, message, .. }`.
- `InteractionData::{ApplicationCommand(Box<CommandData>), MessageComponent(Box<MessageComponentInteractionData>), ModalSubmit(Box<ModalInteractionData>)}`. Autocomplete uses `ApplicationCommand` data with `CommandOptionValue::Focused(String, CommandOptionType)`, possibly nested inside `SubCommand`/`SubCommandGroup` values.
- `InteractionResponseType` (0.17.1): `Pong, ChannelMessageWithSource, DeferredChannelMessageWithSource, DeferredUpdateMessage, UpdateMessage, ApplicationCommandAutocompleteResult, Modal, PremiumRequired (deprecated by Discord)`. **`LaunchActivity` (12) is not modeled**; send it as a raw request if needed.
- `InteractionResponseData` (builder `InteractionResponseDataBuilder`): `allowed_mentions, attachments, choices, components, content, custom_id, embeds, flags, title, tts, poll`.
- Timing: the initial response must arrive within **3 s**, otherwise the token is invalid ("Unknown interaction"). After that the token is valid for **15 min** for `update_response`/followups. Framework rule: auto-defer if a handler has not responded after a configurable threshold (for example 2-2.5 s), measured from receipt in the shard task rather than from handler start, so queueing time counts. Track "responded / deferred" state per interaction so `reply()` turns into `update_response` or a followup transparently.
- Modals (0.17, #2461, breaking): modal top-level components are now `Label { label, description, component: Box<Component> }` wrapping `TextInput`, `SelectMenu` or `FileUpload`, plus `TextDisplay`. `TextInput.label` is **deprecated** in favor of `Label`. The old ActionRow-wrapped text inputs still deserialize. Submit data comes back as `ModalInteractionComponent::{Label, ActionRow, StringSelect, UserSelect, RoleSelect, MentionableSelect, ChannelSelect, TextInput, TextDisplay, FileUpload, Unknown}`.
- `twilight-standby` (`Standby::wait_for_component`, `wait_for_message`, `wait_for_event`, `*_stream`) is fine for "wait for the next button click" flows. Call `standby.process(&event)` on every event. For huge bots, prefer stateless `custom_id` routing (encode the state in the id), because standby futures hold memory and break across restarts or multiple processes.

### 4.3 Sketch: command definitions, autocomplete, defer + followup, modal, V2 response **[compiled, run]**

```rust
//! Sketch 2: commands, interaction responses, Components V2, modals (twilight 0.17.1).
use std::sync::Arc;
use twilight_http::Client;
use twilight_model::{
    application::{
        command::{Command, CommandOptionChoice, CommandOptionChoiceValue, CommandType},
        interaction::{
            application_command::CommandOptionValue, modal::ModalInteractionComponent,
            Interaction, InteractionContextType, InteractionData, InteractionType,
        },
    },
    channel::message::{
        component::{
            ButtonStyle, Component, SeparatorSpacingSize, TextInput, TextInputStyle,
            UnfurledMediaItem,
        },
        MessageFlags,
    },
    guild::Permissions,
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{marker::ApplicationMarker, Id},
    oauth::ApplicationIntegrationType,
};
use twilight_util::builder::{
    command::{CommandBuilder, IntegerBuilder, StringBuilder, SubCommandBuilder, SubCommandGroupBuilder},
    message::{
        ActionRowBuilder, ButtonBuilder, ContainerBuilder, LabelBuilder, SectionBuilder,
        SeparatorBuilder, TextDisplayBuilder, ThumbnailBuilder,
    },
    InteractionResponseDataBuilder,
};

pub fn commands() -> Vec<Command> {
    let config = CommandBuilder::new("config", "Configure the bot", CommandType::ChatInput)
        .default_member_permissions(Permissions::MANAGE_GUILD)
        .contexts([InteractionContextType::Guild])
        .integration_types([ApplicationIntegrationType::GuildInstall])
        .name_localizations([("fr", "configuration")])
        .option(
            SubCommandGroupBuilder::new("logs", "Log settings").subcommands([
                SubCommandBuilder::new("set", "Set log channel")
                    .option(StringBuilder::new("level", "Log level").autocomplete(true).required(true)),
            ]),
        )
        .option(
            SubCommandBuilder::new("limit", "Set a limit")
                .option(IntegerBuilder::new("value", "Value").min_value(1).max_value(100)),
        )
        .validate() // twilight-validate checks (lengths, option ordering, ...)
        .expect("valid command")
        .build();

    let user_ctx = CommandBuilder::new("Inspect", "", CommandType::User)
        .contexts([InteractionContextType::Guild, InteractionContextType::BotDm, InteractionContextType::PrivateChannel])
        .integration_types([ApplicationIntegrationType::GuildInstall, ApplicationIntegrationType::UserInstall])
        .build();
    vec![config, user_ctx]
}

/// Idempotent bulk overwrite (PUT) - call once at deploy, not on every shard READY.
pub async fn register(http: &Client, app_id: Id<ApplicationMarker>) -> anyhow::Result<()> {
    let cmds = commands();
    http.interaction(app_id).set_global_commands(&cmds).await?;
    Ok(())
}

pub async fn on_interaction(http: Arc<Client>, app_id: Id<ApplicationMarker>, i: Interaction) -> anyhow::Result<()> {
    let ic = http.interaction(app_id);
    match (i.kind, &i.data) {
        (InteractionType::ApplicationCommandAutocomplete, Some(InteractionData::ApplicationCommand(data))) => {
            // Find focused option (walk nested subcommand groups in a real impl).
            let _focused = data.options.iter().find_map(|o| match &o.value {
                CommandOptionValue::Focused(v, _) => Some(v.clone()),
                _ => None,
            });
            let choices = ["debug", "info", "warn"].map(|s| CommandOptionChoice {
                name: s.into(),
                name_localizations: None,
                value: CommandOptionChoiceValue::String(s.into()),
            });
            let resp = InteractionResponse {
                kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
                data: Some(InteractionResponseDataBuilder::new().choices(choices).build()),
            };
            ic.create_response(i.id, &i.token, &resp).await?;
        }
        (InteractionType::ApplicationCommand, Some(InteractionData::ApplicationCommand(data))) if data.name == "config" => {
            // 1) ACK within 3 s. Deferred => token valid for 15 min of followups/edits.
            let defer = InteractionResponse {
                kind: InteractionResponseType::DeferredChannelMessageWithSource,
                data: Some(InteractionResponseDataBuilder::new().flags(MessageFlags::EPHEMERAL).build()),
            };
            ic.create_response(i.id, &i.token, &defer).await?;

            // 2) Slow work ... then edit the original response with a Components V2 layout.
            //    NOTE: with IS_COMPONENTS_V2, `content` and `embeds` must be empty.
            let components = components_v2();
            ic.update_response(&i.token).components(Some(&components)).await?;
            // V2 flag is set on creation; for deferred messages put it in the defer's flags
            // or use a followup:
            ic.create_followup(&i.token)
                .flags(MessageFlags::IS_COMPONENTS_V2 | MessageFlags::EPHEMERAL)
                .components(&components)
                .await?;
        }
        (InteractionType::MessageComponent, Some(InteractionData::MessageComponent(data))) if data.custom_id == "open_modal" => {
            let modal = InteractionResponse {
                kind: InteractionResponseType::Modal,
                data: Some(
                    InteractionResponseDataBuilder::new()
                        .custom_id("feedback")
                        .title("Feedback")
                        .components([LabelBuilder::new(
                            "Your feedback",
                            Component::TextInput(TextInput {
                                id: None,
                                custom_id: "text".into(),
                                #[allow(deprecated)]
                                label: None, // deprecated: label lives on the Label wrapper
                                max_length: Some(1000),
                                min_length: None,
                                placeholder: None,
                                required: Some(true),
                                style: TextInputStyle::Paragraph,
                                value: None,
                            }),
                        )
                        .build()
                        .into()])
                        .build(),
                ),
            };
            ic.create_response(i.id, &i.token, &modal).await?;
        }
        (InteractionType::ModalSubmit, Some(InteractionData::ModalSubmit(data))) => {
            for c in &data.components {
                if let ModalInteractionComponent::Label(l) = c {
                    if let ModalInteractionComponent::TextInput(t) = &*l.component {
                        tracing::info!(value = %t.value, "modal text");
                    }
                }
            }
            let resp = InteractionResponse {
                kind: InteractionResponseType::ChannelMessageWithSource,
                data: Some(InteractionResponseDataBuilder::new().content("Thanks!").flags(MessageFlags::EPHEMERAL).build()),
            };
            ic.create_response(i.id, &i.token, &resp).await?;
        }
        _ => {}
    }
    Ok(())
}

pub fn components_v2() -> Vec<Component> {
    let thumb = ThumbnailBuilder::new(UnfurledMediaItem {
        url: "https://cdn.discordapp.com/embed/avatars/0.png".into(),
        proxy_url: None,
        height: None,
        width: None,
        content_type: None,
    })
    .build();
    let container = ContainerBuilder::new()
        .accent_color(Some(0x5865F2))
        .component(SectionBuilder::new(thumb).component(TextDisplayBuilder::new("## Settings\nLogs: **on**").build()).build())
        .component(SeparatorBuilder::new().divider(true).spacing(SeparatorSpacingSize::Small).build())
        .component(
            ActionRowBuilder::new()
                .component(ButtonBuilder::new(ButtonStyle::Primary).custom_id("open_modal").label("Feedback").build())
                .build(),
        )
        .build();
    vec![Component::Container(container)]
}

fn main() {
    let c = commands();
    println!("{}", serde_json::to_string_pretty(&c[0]).unwrap());
    let v2 = components_v2();
    println!("{}", serde_json::to_string(&v2).unwrap());
}
```

Serialized output from running it (abridged). The `"content_type":null` shows the 0.17.1 serialization bug that is fixed on `main` (#2592):

```json
[{"type":17,"accent_color":5793266,"components":[{"type":9,"components":[{"type":10,"content":"## Settings\nLogs: **on**"}],"accessory":{"type":11,"media":{"url":"https://cdn.discordapp.com/embed/avatars/0.png","content_type":null}}},{"type":14,"divider":true,"spacing":1},{"type":1,"components":[{"type":2,"custom_id":"open_modal","label":"Feedback","style":1}]}]}]
```

---

## 5. Components V2 in twilight-model

| Discord type | Component | twilight 0.17.0 / 0.17.1 | `main` (unreleased) | util builder (0.17.0) |
|---|---|---|---|---|
| 1 | Action Row | ✅ `Component::ActionRow` | ✅ | `ActionRowBuilder` |
| 2 | Button (incl. Premium style) | ✅ | ✅ | `ButtonBuilder` |
| 3,5-8 | Select menus | ✅ `SelectMenu` (+`kind`) | ✅ | `SelectMenuBuilder`, `SelectMenuOptionBuilder` |
| 4 | Text Input | ✅ (`label` deprecated) | ✅ | – (struct literal) |
| 9 | Section | ✅ | ✅ | `SectionBuilder` |
| 10 | Text Display | ✅ | ✅ | `TextDisplayBuilder` |
| 11 | Thumbnail | ✅ | ✅ | `ThumbnailBuilder` |
| 12 | Media Gallery | ✅ | ✅ | – (struct literal) |
| 13 | File | ✅ `Component::File(FileDisplay)` | ✅ | `FileDisplayBuilder` |
| 14 | Separator | ✅ | ✅ | `SeparatorBuilder` |
| 17 | Container | ✅ | ✅ | `ContainerBuilder` |
| 18 | Label (modal) | ✅ | ✅ | `LabelBuilder` |
| 19 | File Upload (modal) | ✅ | ✅ | `FileUploadBuilder` |
| 21 | Radio Group (modal) | ❌ → `Unknown(21)` | ❌ | – |
| 22 | Checkbox Group (modal) | ❌ → `Unknown(22)` | ✅ (#2516) | – |
| 23 | Checkbox (modal) | ❌ → `Unknown(23)` | ✅ (#2516, deser fix #2530) | – |

- `MessageFlags::IS_COMPONENTS_V2 = 1 << 15` has existed since **0.17.0** (PR #2422 "Components V2", 2025-11-08). 0.16.x has no V2. Every component has an optional `id: Option<i32>`.
- With `IS_COMPONENTS_V2`, `content`, `embeds`, `poll` and stickers are rejected by Discord. The flag cannot be removed once set. Up to 40 total components are allowed. Attachments must be referenced as `attachment://name` in `UnfurledMediaItem.url`. Neither twilight-validate 0.17.0 nor the builders enforce all of these V2 rules. noctis should validate the total component count, nesting (Container cannot contain Container; Section needs 1-3 TextDisplay plus an accessory) and content/flag exclusivity before sending.
- `Component::Unknown(u8)` drops the payload of unknown types. A **message containing an unknown component cannot round-trip** (for example re-sending or editing a fetched message that contains a checkbox on 0.17.1). The same applies to modal submits: unknown modal components arrive as `ModalInteractionComponent::Unknown(type)` without values.
- **Workarounds for missing types** (Radio Group, or Checkbox on 0.17.1):
  1. Raw JSON through `RequestBuilder::raw(Method::Post, format!("interactions/{id}/{token}/callback")).json(&value).use_authorization_token(false)` plus `client.request::<EmptyBody>(..)` (see the `raw_v2_message` sketch in §3.1).
  2. `.payload_json(bytes)` on `create_message` / `create_followup` / `update_response` builders.
  3. For *receiving* modal submits with unknown components, read the raw gateway JSON. twilight does not expose it after parsing, so noctis would have to wrap `twilight_gateway::parse` itself: receive `Message::Text(String)` via `Shard::poll_next`, peek the `t`/`type`, and for `INTERACTION_CREATE` modal submits deserialize with noctis's own `serde_json::Value` fallback. This is the reason to build the event loop on `Stream<Item = Message>` + `twilight_gateway::parse` rather than `next_event`.
  4. Track or upstream PRs: twilight accepts model PRs quickly (see the October 2026 commit burst).

---

## 6. Performance for very large bots

1. **Filter before parsing.** Use narrow `EventTypeFlags`, because unwanted events never reach serde. Pair them with minimal intents (each intent you drop is bandwidth, decompression CPU and parse CPU saved).
2. **Feature `simd-json`** on gateway and http (simd-json 0.17, `serde_impl`, `swar-number-parsing`). It parses in place on the owned `String`, which is why `parse()` takes `String` and the `Deserializing` error notes "simd-json may slightly modify the event". Benchmark on your target CPU (AVX2), because the gains are biggest for large payloads (GUILD_CREATE). Keep `zstd` compression.
3. **Allocation:** each event is one decompressed `String` plus the model allocations. Big variants (`GuildCreate`, `MessageCreate`, `InteractionCreate`, ...) are boxed, so `Event` itself is 312 B to move. Avoid cloning `Event`s and pass `Arc<Event>` if several consumers need the same event. Use a global allocator tuned for many threads (mimalloc or jemalloc) at the binary level and let noctis document it, without forcing it. `perf(model): monomorphize IdVisitor` (#2559, main) reduces code size.
4. **Spawn per event vs worker pool.** `tokio::spawn` per event costs about 1 allocation plus a scheduler push. That is fine for interactions (low volume) but wasteful for MESSAGE_CREATE / PRESENCE at millions of guilds. Recommended: the shard task does only `cache.update` (sync, cheap) plus dispatch into a **bounded** channel. A fixed pool of N workers (≈ cores × k) handles it, hashing by guild_id when ordering matters. Interactions get spawned (or go to a priority pool) because of the 3 s deadline.
5. **Backpressure:** a bounded `send().await` in the shard task stops reading that shard's socket. Short stalls are fine, but long ones cause Discord to drop the connection (missed heartbeat ACK and zombie detection), followed by resume and replay. Policy options: (a) block (correctness), (b) `try_send` and drop low-value events (typing, presence) with a metric, (c) a separate high-priority channel for interactions that is never dropped. Export per-shard queue depth and `shard.latency()` metrics.
6. **Runtime:** one multi-thread runtime is fine. The upstream `bucket()` docs on `main` suggest thread-per-core shard buckets for extreme scale. Keep JSON parsing on the shard task (that parallelizes across shards) and do not funnel all shards through one task.
7. **HTTP:** one `Client` per process. The ratelimiter is a single actor task per `Client`, so at very high request rates use the external proxy and `ratelimiter(None)`, which also enforces the shared global limit. Use `set_pre_flight` to drop obsolete edits.
8. **Startup:** set `InMemoryQueue::new(max_concurrency, ..)` correctly. Persist sessions so restarts RESUME. Stagger process starts by bucket.

---

## 7. Gotchas & breaking changes

**0.15 → 0.16 (2025-01-12)**
- The `Queue` trait was rewritten (`enqueue(id) -> oneshot::Receiver<()>`) and `Shard<Q>` became generic. `Shard` implements `Stream` (`next_message` was replaced by `StreamExt::next`/`next_event`). WebSocket errors are swallowed and reconnected internally.
- Feature renames: `native` → `native-tls`. bitflags v2 (`Intents`, `Permissions`, `EventTypeFlags` are now bitflags-2 types). Deprecated APIs and undocumented events removed.
- In-memory cache became generic (`InMemoryCache<CacheModels>`, use `DefaultInMemoryCache`), and `InMemoryCache::new()` returns the default.
- HTTP error type changes, select menu unification (`SelectMenu { kind, .. }`).

**0.16 → 0.17 (2025-11-08)**
- `twilight-http-ratelimiting` was **rewritten**: the `Ratelimiter` trait is gone, replaced by the concrete actor `RateLimiter`. `ClientBuilder::ratelimiter(Option<RateLimiter>)`, unknown-path support, and retry-delay logic removed. `ResponseFuture` **auto-retries 429s**.
- Gateway: `zstd-stream` compression added and made default. `zlib` was updated and un-deprecated (breaking feature changes). Dependency bumps (rustls 0.23, tokio-websockets) mean **you must install a rustls CryptoProvider**. MSRV is 1.89.
- Model: Components V2 (`Component` gained many variants, so exhaustive matches break), new modal components (`Label`, `FileUpload`; `TextInput.label` deprecated), role colors / primary guild (breaking struct changes), `NonZero` type alias migration (`NonZeroU64` → `NonZero<u64>`), and `GatewayHeartbeat` incoming data is `null`.
- HTTP: `create_guild` endpoint removed, new pin endpoints (`get_channel_pins` paginated), `with_response()` on create_response.
- `twilight-http-ratelimiting` 0.17.0 has a panic bug. **0.17.1 is required.**

**0.17.1 → next (on `main`, likely 0.18)**
- `create_bucket` and `create_recommended` are deprecated, the gateway `twilight-http` feature is deprecated, and `bucket()` returns `ShardId`s.
- `MessageCreate` gained `channel_type` (breaking). `create_response_with_response` returns `InteractionCallbackResponse` (breaking). Create-role icon is serialized as a string. Checkbox components were added (another `Component` variant, so non-`_` matches break).
- `validate`: string lengths now always use char count (#2586, #2555, #2557). Text that 0.17 accepted or rejected by byte length behaves differently.

**Evergreen gotchas**
- `Client::new`, `RateLimiter::new` and `InMemoryQueue::new` panic outside a tokio runtime.
- `InMemoryQueue::default()` uses `max_concurrency = 1`.
- `CloseFrame::NORMAL` invalidates the session. Use `CloseFrame::RESUME` for restarts.
- Cache `Reference` guards must not cross `.await`.
- `Component::Unknown`, `CommandType::Unknown`, `Event` deserialization errors for brand-new events: twilight lags Discord by weeks or months. Design noctis's public types to not leak twilight enums in exhaustive form, and keep a raw-JSON escape hatch.
- twilight states that the **MSRV may change in semver-compatible releases** (#2408).
- `twilight-lavalink` on crates.io targets Lavalink v3, which modern Lavalink servers (v4) no longer serve. Use `lavalink-rs` 0.15 or wait for the twilight-lavalink v4 release. Use `songbird` for native voice.

---

## 8. Recommendations for noctis

1. **Pin `0.17.1`** for gateway, http, model, cache and ratelimiting, and `0.17.0` for util, validate, standby, mention and queue. Re-export `twilight_model` (and possibly `twilight_http`) from noctis so users never get version skew. Plan a migration to 0.18 when it ships. Keep noctis's code on APIs that survive it: build shards from consecutive `ShardId` ranges yourself, and do not use `create_bucket`/`create_recommended`.
2. **Bootstrap owns the hazards.** Install the rustls provider, build the tokio runtime before any twilight constructor, fetch `GET /gateway/bot` and the application id once, and configure `InMemoryQueue` with the real `max_concurrency`.
3. **Sharding:** one tokio task per shard. A `ShardManager` holds `Vec<MessageSender>`, latency and state per shard, and supports `ClusterConfig { cluster_id, cluster_count, total_shards }` with consecutive, `max_concurrency`-aligned ranges. Provide a `Queue` trait implementation over a pluggable `IdentifyCoordinator` (in-memory, HTTP, redis) for multi-process use.
4. **Event pipeline:** shard task → `parse` with computed `EventTypeFlags` (noctis derives them from registered handlers and enabled cache resources, so users never touch flags) → cache update → dispatch. Interactions go to a never-dropping fast path with spawn per interaction. Everything else goes to a bounded, guild-hashed worker pool with a configurable overflow policy (block / drop-low-priority). Consider consuming `Message` via `Stream` and calling `twilight_gateway::parse` directly so noctis can keep a raw-JSON fallback for unknown events and components.
5. **Graceful shutdown and rolling restarts:** close with `CloseFrame::RESUME`, drain until `GatewayClose`, persist `Session` + `resume_url` per shard through a `SessionStore` trait (file / redis), and restore on boot.
6. **HTTP:** one `Arc<Client>` in the context. Support a `proxy` mode that disables the local ratelimiter, and make it the documented default for clustered deployments. Set the timeout to about 10-15 s. Expose `raw()` helpers built on `RequestBuilder::raw` for unmodeled endpoints.
7. **Interactions layer (noctis's core value):**
   - Derive or attribute macros that produce `twilight_model::application::command::Command` (use the util builders internally plus `.validate()`), including localizations, `contexts`, `integration_types`, `default_member_permissions` and `nsfw`.
   - A hash-checked bulk sync (`set_global_commands` / `set_guild_commands`) run once per deployment, not per shard.
   - A response context with a state machine (Unacked → Deferred / Responded → Followups). Auto-defer at a configurable threshold before 3 s, measured from gateway receipt. Make `reply()` choose `create_response`, `update_response` or `create_followup`. Warn when the 15 min token expiry approaches.
   - Typed `custom_id` routing for components and modals (stateless, multi-process safe). `twilight-standby` stays optional for short-lived flows.
   - Autocomplete helpers that find the focused option through nested subcommand groups.
8. **Components V2:** ship noctis's own V2 builder or `view!` macro that outputs `twilight_model` components, validates V2 rules (40 components, nesting, flag/content exclusivity, attachment refs) and sets `IS_COMPONENTS_V2` automatically. Have a raw-JSON fallback for Radio Group, Checkbox and Checkbox Group until twilight releases them (Checkbox is on `main`).
9. **Cache:** a `CacheBackend` trait plus implementations: `NoCache`, `InMemory` (twilight with slim custom `CacheableModels`), and an optional `noctis-cache-redis` crate. Default to `GUILD | CHANNEL | ROLE | USER_CURRENT`, no members, no messages. Tie resource selection to intents and event flags automatically. Never expose DashMap guards across `.await` (return owned or `Arc` values).
10. **Performance features:** expose `simd-json` and allocator recommendations as opt-in noctis features. Add a `metrics` feature with per-shard latency, queue depth, dropped events and HTTP 429 counts.
11. **Don't depend on `twilight-lavalink`** until its v4 release. Leave voice to `songbird` and `lavalink-rs` integrations.
12. **Naming:** publish as `noctis` / `noctis-*` (free on 2026-10-19; see top).
