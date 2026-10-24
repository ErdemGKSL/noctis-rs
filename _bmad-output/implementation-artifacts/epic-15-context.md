# Epic 15 Context: Opt-in extras

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Give bot authors four opt-in P2 capabilities on top of the v1 core: a replaceable Renderer seam (so rendering can later move to a remote service), several Apps with different tokens in one process (white-label bots), presence and gateway commands, and prefix message commands that reuse slash handlers. Each ships behind a cargo feature or as an extension and may slip without blocking v1.

## Stories

- Story 15.1: Renderer seam
- Story 15.2: Multiple Apps per process
- Story 15.3: Presence and gateway commands
- Story 15.4: Prefix commands

## Requirements & Constraints

- Renderer seam: a test Renderer (fixed bytes) replaces the local one via `.renderer(..)` with no change to Templates, Views or handlers; cache key, attachment naming and Upload Budget still apply to custom output. v1 ships only the local implementation; renderer types are exposed only through `noctis::image::unstable`.
- Multiple Apps: same Module tree mountable on several Apps; a Command can be restricted to a subset of Apps for Publish; a `GuildAccessList` allow/deny list (resolved through an Adapter) lets a white-label App serve only its customer's guild while the main App skips it at pipeline stage 3. Adapters are shared only where the author chooses.
- Presence/gateway: initial presence used at identify; runtime update for all shards or one. Commands for a guild outside this process's Shard Range, the gateway send limit (120 per 60 s per shard, GW-04) and request-guild-members rate limiting (`Event::RateLimited`, GW-07) are typed errors, never silent drops. `LargeThreshold::new::<N>()` outside 50–250 fails to compile (GW-06, compile_fail fixture).
- Prefix: `prefix` feature off by default; off means no prefix parsing code compiled and no message-content intent needed. On, `<prefix><Command Path> <args>` runs the same handler; arg parse failures yield the same typed errors as slash options. Building with prefix Commands but without message-content intent fails (GW-05). `prefix` alone and with all features must build and test (cargo hack matrix).
- Every story's public API change updates the project template, its AGENTS.md and Usage Skill, re-blesses the API snapshot, and passes `skill-check`.

## Technical Decisions

- Render contract (`Renderer`, `RenderJob`, `RenderOutput`) lives in `noctis-ports`; core never depends on `noctis-image`. Attachments are content-addressed; `ctx.render` builds them via `Attachment::rendered(bytes, digest, ext)`; renders are awaited inside handlers.
- `noctis::Runtime` (in `noctis-core`) owns process-scoped resources (metrics recorder, type map via `Runtime::resource_or_init::<T>`; render pool, takumi caches, fonts). `Runtime::global()` is default; `AppBuilder::runtime(&rt)` attaches another; first App's config freezes a resource, later conflicts are `BuildError`. `Runtime::for_test()` gives isolated runtimes.
- Per App: its own single `Arc<twilight_http::Client>` (through the internal edge forwarder), shards, Registry, Responder, `Limits` and namespaced ports; metrics carry an `app` label. No other crate constructs a Discord HTTP client.
- Presence and gateway commands live in `noctis-gateway` and go to the owning shard's sender.
- Feature crates enter core only via `trait Plugin { fn install(&self, app: &mut AppBuilder) }` and `AppBuilder::plugin(..)`. Prefix lives in `noctis-prefix` (`PrefixPlugin::new("!")`), mounts a `MESSAGE_CREATE` Event Handler and calls `Dispatcher::run_command` (from `EventCtx::dispatcher()`), which runs pipeline stages 3–10 and replies through the message-reply Responder. Core contains no prefix code.
- Intents are derived from registered handlers; a handler needing an unrequested intent is a `BuildError`.

## Cross-Story Dependencies

- 15.1 builds on the Renderer port (Epic 3) and image pipeline (Epic 10).
- 15.2 relies on `Runtime`/plugins, Publish (Epic 6) and the fixed interaction pipeline stage 3.
- 15.3 extends the sharded gateway (Epic 5) and rules catalog (`docs/discord-rules.md` + `noctis-rules`, same rule ids).
- 15.4 depends on `Dispatcher::run_command` (Story 6.5) and the option decoding/typed errors of Epic 6; it is the last story, so its gates include `cargo hack --each-feature`, and the launch feature matrix (Epic 14) must add `prefix`.
