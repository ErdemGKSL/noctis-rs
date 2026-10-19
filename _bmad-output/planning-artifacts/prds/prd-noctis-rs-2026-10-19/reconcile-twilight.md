# Reconciliation: PRD vs `research/twilight-rs.md`

**Date:** 2026-10-19. **PRD:** `prd.md` (draft). **Input:** `../../research/twilight-rs.md` (twilight 0.17.1 + `main` @ `6a540ec`).
Scope: capabilities and constraints the PRD misses or states wrongly. Pure implementation detail (task layout, channel types, crate wiring) is left to architecture.
Severity: **High** = PRD requirement can't be met as written, or a missing requirement causes outages at scale; **Med** = missing requirement with user-visible effect; **Low** = clarification.

---

## T-1 (High) twilight can't model Radio Group, and Checkbox/Checkbox Group only exist on unreleased `main`

**Research (§5):** On 0.17.1, types 21/22/23 deserialize as `Component::Unknown(u8)` / `ModalInteractionComponent::Unknown(type)` **with the payload dropped**. Radio Group isn't on `main` either. Consequences: (a) sending needs raw JSON (`RequestBuilder::raw` or `.payload_json`); (b) **receiving** modal submit values for these types is impossible through twilight's parsed `Event`, so Noctis must read the raw gateway JSON (consume `Message` from the shard stream and call `twilight_gateway::parse` itself, keeping a raw fallback); (c) a fetched message containing an unknown component can't round-trip on edit.
**PRD:** FR-29 promises Checkbox in modals with a typed struct; FR-31's escape hatch covers outbound JSON only.

**Suggested FR-31 text:**
> A bot author can include raw component JSON for component types twilight doesn't model yet, and Noctis can receive them.
> **Consequences (testable):**
> - Raw JSON is counted by the validator.
> - Modal submits containing Radio Group, Checkbox Group or Checkbox decode into the typed submission struct on the supported twilight version (via a raw-JSON path when twilight drops them).
> - Editing a View never re-sends a component Noctis couldn't parse; the edit uses the View's own rendered tree, not the received one.

## T-2 (High) Multi-process Clusters need a shared identify queue

**Research (§1.2, §1.6):** The identify queue must serialize identifies **per bucket across all processes**, because one token has one `max_concurrency`. `InMemoryQueue` is per process. Upstream has an HTTP-queue example; redis also works. Ranges should be consecutive and aligned to multiples of `max_concurrency`.
**PRD:** FR-3 lets each process run a Shard Range but says nothing about coordinating identifies. Running UJ-3's four processes with independent in-memory queues violates Discord's identify concurrency and triggers invalid-session / identify-quota failures. No Adapter covers this.

**Suggested FR-3 consequences + new Adapter:**
> - All processes of a Cluster identify through one shared identify coordinator, so per-bucket identify concurrency holds Cluster-wide.
> - A Shard Range whose bounds are not aligned to `max_concurrency` is accepted but logged as a warning.
> - FR-38 gains an **IdentifyQueue** Adapter (in-memory default for single-process Apps; the conformance suite of FR-39 covers it).

## T-3 (High) HTTP rate limits are per token: Clusters need a shared HTTP proxy mode

**Research (§1.6, §2):** The global limit (50 req/s by default) and per-route buckets are **per token, not per process**. In 0.17, `ResponseFuture` **retries 429s immediately and indefinitely** and the old retry-delay logic was removed, so running without a correct limiter is dangerous. Supported setup: twilight-http-proxy or nirn-proxy with `ClientBuilder::proxy(url, true).ratelimiter(None)`. Interaction and webhook endpoints bypass the global limit.
**PRD:** No requirement. RateLimitStore (FR-46) is for user-facing command limits, not Discord HTTP.

**Suggested new FR (in 4.1):**
> **FR-x: Shared HTTP rate limiting.** A bot author can point the App's HTTP client at an external rate-limit proxy, which disables the local limiter.
> - The Cluster deployment guide and Project Template configuration make proxy mode the documented default when more than one process shares a token.
> - Without a proxy, a multi-process App logs a warning at startup.
> - HTTP request timeout is configurable (default 10-15 s).

## T-4 (Med) FR-1 startup panics: list is incomplete, and invalid tokens

**Research (§2, §7):** Panics: `Client::new`, `RateLimiter::new` and `InMemoryQueue::new` outside a tokio runtime; `Client::new` without a rustls `CryptoProvider`. `twilight-http-ratelimiting` **0.17.0 can panic** (fixed in 0.17.1). After a 401 the client remembers the invalid token and fails every later request fast. `next_event` returning `None` means a fatal close (bad token, invalid or disallowed privileged intents).
**PRD:** FR-1 covers runtime and TLS only.

**Suggested FR-1 consequences:**
> - An invalid token, or intents Discord rejects (e.g. an unapproved privileged intent), fails startup with a typed error naming the cause; a fatally closed Shard after startup is surfaced as a shard-status event and metric, not a silent stop.
> - NFR-7: the workspace pins `twilight-http-ratelimiting ≥ 0.17.1`.

## T-5 (Med) FR-4 session resume: the close code and resume URL matter

**Research (§1.4):** `CloseFrame::NORMAL` (1000/1001) **invalidates** the session; only `CloseFrame::RESUME` (4000) keeps it. Resuming needs both `Session` (id + sequence) **and `resume_url`**. After `close`, the shard must be polled until `GatewayClose` so the final sequence is saved. Sessions are kept only briefly by Discord.
**PRD:** FR-1 "Shards close" and FR-4 "SessionStore" don't state this, so a normal graceful shutdown would make FR-4 impossible.

**Suggested FR-1 / FR-4 consequences:**
> - A graceful shutdown closes Shards with a resumable close and stores each Shard's session id, sequence and resume URL in the SessionStore before exiting.
> - A non-resumable shutdown (e.g. `--no-resume`) is available and clears the stored sessions.

## T-6 (Med) FR-5 backpressure: blocking stalls the socket; interactions need a priority lane

**Research (§1.8, §6.4-6.5):** A bounded `send().await` in the shard task stops reading that shard's socket; long stalls cause Discord to drop the connection (missed heartbeat ACK / zombie), then resume and replay. Interactions have a 3 s deadline and must never be dropped or queued behind bulk events. Optional per-guild ordering via guild-hashed workers. The auto-defer deadline should be measured **from gateway receipt**, not from handler start.
**PRD:** FR-5 requires "nothing is silently dropped" and "a sleeping handler doesn't delay heartbeats", but blocking the shard on a full queue contradicts the second over time. FR-21 doesn't say where the 2.5 s is measured from.

**Suggested FR-5 consequences:**
> - Interactions use a separate, never-dropped dispatch path that is not blocked by queued gateway events.
> - The overflow policy for other events is configurable: block (default) or drop configured low-priority event types, with a dropped-events metric. Nothing is dropped silently.
> - A handler set can opt into per-guild ordered dispatch.

**Suggested FR-21 consequence:**
> - The acknowledgement deadline is measured from the time the gateway event was received, so queueing time counts.

## T-7 (Med) Intents, event filters and cache must agree

**Research (§1.7, §3):** Intents decide what Discord sends; `EventTypeFlags` decide what twilight deserialises; cache `ResourceType`s need the matching intents (a member cache without `GUILD_MEMBERS` is useless). Privileged intents dominate bandwidth/CPU/memory; an interaction-first default is `Intents::GUILDS`. The Intents → EventTypeFlags helper was reverted upstream, so Noctis owns the mapping.
**PRD:** FR-6 derives event filters; FR-40 selects cache resources; nothing ties them to intents or sets a default.

**Suggested FR-40 consequences:**
> - The default intents are the minimum required by registered Event Handlers and enabled cache resources (`GUILDS` for an interaction-only bot).
> - Startup fails with a typed error when a registered Event Handler or cache resource needs an intent that isn't enabled.
> - Cache reads return owned or shared values; no cache guard is exposed that could be held across an `.await`.

## T-8 (Med) Resharding is unaddressed

**Research (§1.5):** Large bots must periodically re-check `GET /gateway/bot` and reshard by starting a new shard set alongside the old one (duplicate events during overlap). Large-bot sharding requires a shard count that is a multiple of `max_concurrency` (16).
**PRD:** FR-2 fixes the count at startup; no resharding and no "multiple of max_concurrency" rule.

**Suggested:** either a new FR ("An App can reshard without downtime when Discord's recommended count grows; during overlap, handlers see each event once") or an explicit §8.2 deferral. In both cases add to FR-2:
> - A configured shard total that isn't a multiple of `max_concurrency` (when `max_concurrency > 1`) is a startup warning.

## T-9 (Med) Gateway commands and presence

**Research (§1.3):** Presence updates, request guild members and voice state are sent per shard through a `MessageSender`, rate-limited to 120 per 60 s per shard. 0.17.1 adds `Event::RateLimited` for request-guild-members.
**PRD:** No FR for setting presence/activity or sending gateway commands, both common needs.

**Suggested new FR (in 4.1):**
> **FR-x: Presence and gateway commands.** A bot author can set an initial presence and update it at runtime for all Shards or one Shard, and can send gateway commands (e.g. request guild members) to the Shard owning a guild. Gateway rate-limit responses are surfaced as typed errors.

## T-10 (Low) Command Publish constraints

**Research (§4.1):** Publish should use the bulk-overwrite PUT, run once per deployment (never per shard READY); global command creates are limited to 200 per day per application; twilight-validate enforces the 8000-char total per command (counted in chars since #2555) and option ordering (required before optional).
**Suggested FR-7 / FR-13 consequences:**
> - Registration fails with a typed error when a Command exceeds the 8000-character total or puts an optional option before a required one.
> - Publish never runs on shard READY; it runs only in the designated process or CLI mode.

## T-11 (Low) Autocomplete inside subcommand groups

**Research (§4.2):** The focused option may be nested inside SubCommand/SubCommandGroup values.
**Suggested FR-10 consequence:** An autocomplete handler for an option of a 3-word Command Path receives the focused value and the other filled options as a typed partial struct.

## T-12 (Low) Version policy: 0.18 and MSRV drift

**Research (§0, §7, §8):** `main` carries breaking changes (likely 0.18): `create_bucket`/`create_recommended` deprecated, `MessageCreate.channel_type`, `InteractionCallbackResponse`, char-count validation. twilight says its **MSRV may change in semver-compatible releases**. 0.17.1 serialises `UnfurledMediaItem.content_type` as `null` (fixed on `main`, #2592).
**PRD:** Open question 3 covers 0.17 vs 0.18; §6 re-exports twilight types.
**Suggested NFR-7 / §6 addition:**
> - Public Noctis types don't expose twilight enums in a way that forces exhaustive matches, so new twilight variants are not breaking for Noctis users.
> - Noctis's V2 serialisation omits unset optional media fields regardless of the twilight version.

## T-13 (Low) Raw HTTP escape hatch

**Research (§2):** `RequestBuilder::raw` goes through the same ratelimiter and retries; needed for unmodeled endpoints (LaunchActivity, Primary Entry Point).
**Suggested FR-31 consequence:** A bot author can send a raw REST request through the App's HTTP client and rate limiter.

## T-14 (Low) Memory reference numbers for NFR-1

**Research (§3):** Measured `size_of`: CachedGuild 440 B, Member 176 B, Channel 1224 B, Message 3520 B; a lean `GUILD | CHANNEL | ROLE` cache with slim models is a few hundred bytes to ~2 KB per guild. Members dominate.
**Suggested:** NFR-1 states the default cache configuration used for the benchmark (no members, no messages) and reports twilight cache memory alongside framework memory, so SM-C2 can be checked.
