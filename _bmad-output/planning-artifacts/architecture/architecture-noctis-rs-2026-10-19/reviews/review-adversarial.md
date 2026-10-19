---
type: architecture-review
lens: adversarial (compatible-by-the-letter, incompatible-in-fact)
target: ../ARCHITECTURE-SPINE.md
prd: ../../../prds/prd-noctis-rs-2026-10-19/prd.md
reviewed: '2026-10-19'
verdict: 'Not ready to bind. 17 ADs hold the crate graph and wire format, but they leave 6 blocking seams and 9 major ones open.'
---

# Adversarial Review: Noctis Architecture Spine

## Verdict

**Not ready to bind.** The spine is strong on single-crate properties: the codec format,
the rules crate, the dependency DAG and the port shape. Where it is weak is the
**seams between crates**. Several FR-level behaviors span crates that the DAG forbids
from seeing each other, and the spine never says which side owns them. A `noctis-ui`
author and a `noctis-core` author can each follow every AD exactly and still ship
crates that don't fit together. That happens most clearly in five places: Ref
creation, widget navigation Routes, attachment naming, SessionStore writes on shutdown,
and port key namespacing.

Method: for each seam, I built two implementations that each satisfy every AD as
written and then showed the clash between them. Every hole ends with the exact AD text
that closes it. Severity is graded as follows. **Blocking** means the units won't
integrate, or a security or correctness bug is guaranteed. **Major** means they
integrate but behave inconsistently, or need a cross-team rewrite. **Minor** means an
ambiguity that will cost a review cycle.

| # | Seam | Severity | Fix |
|---|------|----------|-----|
| H1 | ui ↔ core ↔ ports: who creates a Ref (spill/by_ref) from inside a pure View | Blocking | new AD-18 |
| H2 | ui ↔ codec ↔ core: where the `Route` trait lives (FR-58 typed handle) | Blocking | new AD-19 |
| H3 | ui ↔ core ↔ macros: widget navigation Routes and how page, tab, state-version and nonce are encoded | Blocking | new AD-20, tighten AD-6 |
| H4 | codec ↔ ports: Ref value envelope (route binding, schema version, one-shot atomicity) | Blocking | tighten AD-5, AD-14 |
| H5 | gateway ↔ core: shutdown/resume ordering and the single SessionStore writer | Blocking | new AD-21 |
| H6 | ports wrapper ↔ adapters: who applies `<app>:` namespacing; which ports are "KV" | Blocking | tighten AD-14 |
| H7 | ui ↔ image ↔ core: attachment naming, diffing on edit, Upload Budget check | Major | new AD-22 |
| H8 | ui ↔ image ↔ core: async render inside a sync View | Major | fold into AD-22 |
| H9 | codec ↔ macros: who computes field widths and escape expansion | Major | tighten AD-6 |
| H10 | AD-5 `<ver>`: wire-format version or per-Payload schema version | Major | tighten AD-5 |
| H11 | i18n ↔ ui ↔ macros: localizing framework chrome; Locale type ownership; command-name source | Major | new AD-23 |
| H12 | core: interaction pre-handler pipeline order (guards, rate limit, module-disabled, middleware, auto-defer) | Major | new AD-24 |
| H13 | Middleware Extensions: collisions, missing `Ext<T>` | Major | tighten AD-9 / AD-24 |
| H14 | gateway ↔ core: lane item shape (raw JSON, receipt instant) and lane ownership | Major | tighten AD-10 |
| H15 | gateway ↔ core: who owns the HTTP client (FR-52 "no request bypasses") | Major | new AD-25 |
| H16 | multi-App: process-global resources (render pool, takumi caches, metrics labels) | Major | new AD-26 |
| H17 | AD-15 cache key vs FR-36 (missing versions, theme) | Major | tighten AD-15 |
| H18 | `Limits` instance threading (default vs overridden) | Minor (silent) | tighten AD-3 |
| H19 | AD-7 "never rebuilds" vs "swap with arc-swap" | Minor | tighten AD-7 |
| H20 | macro-generated path roots (`::noctis` vs sub-crate) | Minor | tighten AD-2 |
| H21 | `HandlerError` user message key type | Minor | tighten AD-13 |
| H22 | three meanings of "rate limit" vs AD-1 | Minor | tighten AD-1 |

---

## Blocking holes

### H1: Who creates a Ref when a pure View emits a spill or by_ref component

**Two compliant units.**
- *ui author:* the DAG says `noctis-ui → codec, rules`, with no ports and no I/O
  (Layer table). A View with a `#[route(by_ref)]` button therefore can't call
  `RefStore::put`. The author makes `Button::new(route, payload)` call
  `codec::encode` and, for by_ref, write a fresh Ref string into the `custom_id`.
  The payload bytes are then simply dropped, because there is nowhere to put them.
- *core author:* the Responder (AD-11) receives a finished component tree, which only
  contains strings. Every Ref in it is dangling, and every click goes to the Expired
  Handler.
- AD-5 adds that "the codec is the only code that builds a custom_id". So core can't
  patch strings after the fact either. Both sides are compliant, and the result
  never works.

The same problem hits FR-27 ("Cancel … deletes the Wizard's Refs"), because the Wizard
lives in a crate that can't reach the RefStore.

**Proposed AD-18: Deferred Refs; the Responder materializes them.**
> - `noctis-codec` defines `EncodedId`, an enum of `Inline(AsciiString)` and `PendingRef { route: RouteId, schema: u8, bytes: Bytes, ttl: Option<Duration>, one_shot: bool }`. Pure crates (`noctis-ui`, widgets) only ever produce `EncodedId`; they never invent Ref characters.
> - Before validation and send, `noctis-core`'s Responder walks the tree, `put`s each `PendingRef` through `RefStore`, and asks the codec to produce the final `custom_id` (`codec::finish(PendingRef, RefKey) -> AsciiString`). Identical `PendingRef`s within one message share one Ref.
> - Ref deletion requested by pure code (Wizard cancel, one-shot consumption) is expressed as a returned `RefOps` list on the response value; only core executes it.
> - No crate below `noctis-core` holds a `RefStore` handle.

### H2: Where the `Route` trait lives

FR-58 says "a component is built from its Route's typed handle". `#[derive(Route)]`
(AD-6) emits a const assert, but no AD says which crate defines `trait Route`.
- *core author* puts `Route` next to `Registry` in `noctis-core` (it is the dispatch
  key). The DAG then forbids `noctis-ui` from naming it, so typed component
  construction is impossible.
- *ui author* defines `ui::RouteRef<P>`. The macro author emits impls against core's
  trait. The result is two Route abstractions and a macro that can implement only one
  of them.

**Proposed AD-19: Route identity lives in the codec.**
> - `noctis-codec` defines `trait Route { const ID: RouteId; type Payload: Payload; const MODE: RouteMode /* Inline | ByRef | Spill */; const MAX_ID_LEN: usize; }`. `#[derive(Route)]` implements exactly this trait and nothing else.
> - `noctis-ui` builds components only from `R: Route` (+ `R::Payload` value) and returns `EncodedId` (AD-18).
> - `noctis-core` binds handlers to routes with a separate descriptor keyed by `R::ID`; the App-build check "component Route without handler" (FR-58) is the set difference between Routes referenced by registered Views/widgets (collected by descriptors) and Routes with handlers.

### H3: Widget navigation Routes (Paginator, Navbar, Wizard, View state version, select nonce)

The PRD says page, tab and step "travel in the Payload" and that switching stores
nothing (FR-26, FR-28). It doesn't say *whose* Payload or *whose* Route. Here are two
compliant designs:
- *Design A (ui author):* the Paginator registers its own Route `pg` with payload
  `(page: u16, view_key: u32)` and looks up the author's View by key. That needs a
  framework-owned Route and a View registry. It also collides with any bot that picks
  the route id `pg`, because AD-5 says route ids are unique per App and leaves no
  reserved namespace.
- *Design B (core author):* navigation clicks go to the **author's** View Route, with
  page and tab appended to the author's Payload.

The two designs can't coexist. Design B also breaks AD-6: `#[derive(Route)]` proves
`ver + route + 1 + max ≤ 100` for the author's Payload **only**. The widget fields, the
FR-25 state version, the addendum's per-render select nonce and the optional FR-19
integrity tag are all added at run time. That turns a "proven" Route into one that
overflows in production, which is exactly what AD-6 exists to prevent.

The jump control (FR-28, more than 5 pages) adds a further problem. If it is a modal,
the Route must be declared modal-opening (AD-11), and the author never wrote that Route.

**Proposed AD-20: Widget state is a reserved envelope on the View's own Route.**
> - Widgets never register Routes of their own. Every widget control (page nav, jump, tab, wizard next/back/cancel) encodes to the **owning View's Route**.
> - The codec reserves a fixed-width **view envelope** before the author's fields: `state_ver: u8, tab: u8, page: u16, nonce: u8` (base62, fixed widths), present on every Route that is a View Route (`#[route(view)]`), absent otherwise. A 1-char action discriminator selects the widget action (`p` page, `t` tab, `n`/`b`/`c` wizard, `j` jump, `_` author).
> - The FR-19 integrity tag, when enabled, is a fixed `TAG_LEN` suffix that is always reserved in the bound, even when off.
> - `#[derive(Route)]`'s assert becomes `ver + route + 1 + ENVELOPE_LEN(if view) + max + TAG_LEN ≤ 100`; the codec exports `ENVELOPE_LEN` and `TAG_LEN` as consts.
> - Jump-to-page uses a String Select (no modal), so View Routes are never modal-opening by virtue of widgets.
> - Route ids beginning with `nx-` are reserved for the framework; `#[route]` rejects them in user code (rule id assigned in `docs/discord-rules.md`).

### H4: The Ref value envelope (route binding, stale schema, one-shot)

- `custom_id`s are client-controlled (FR-19). Suppose an attacker clicks
  `<v>admin-ban:~<ref minted for route "poll-vote">`. The RefStore returns the bytes,
  and core postcard-decodes them as `AdminBanPayload`. Postcard is not
  self-describing, so a decode into the wrong type can succeed and produce garbage
  values. The Ref has the required entropy, but it isn't **bound** to a Route.
- FR-60 Stale Payload detection relies on the schema version in the `custom_id`.
  For a Ref, the `custom_id` carries only `~xxxx`, so the schema version has to live
  in the stored value. No AD says so. The adapter author stores raw postcard, and the
  core author expects a version byte.
- One-shot Refs (FR-17) implemented as `get` and then `delete` on a shared Redis are
  non-atomic, so two processes can both consume the Ref. AD-14 lists only "keys plus
  `Bytes` with TTL", which leaves out `take`.

**Tighten AD-5, adding:**
> - A Ref resolves to a stored `RefRecord { route: RouteId, schema: u8, payload: Bytes }`, encoded by `noctis-codec` (not postcard of the user type alone). On resolve, core rejects a record whose `route` differs from the `custom_id`'s route (treated as forged → unknown-Route path, FR-19) and routes a `schema` mismatch to the stale/Expired handler (FR-60).

**Tighten AD-14, adding:**
> - `RefStore` has `put`, `get`, `take` (atomic get-and-delete, required for one-shot), `delete` and `touch`. Conformance suites test `take` under concurrent callers: exactly one wins.

### H5: Shutdown and resume, with two owners of the SessionStore

The DAG has `core → gateway → ports`. FR-1, FR-54 and FR-55 together need this sequence:
"shutdown started" hook, shards close with 4000, the shard is polled until
`GatewayClose`, the session is persisted, in-flight handlers drain within the grace
period, then the process exits. Two compliant builds:
- *gateway author:* `Shard` drop or close persists to `SessionStore` (the gateway
  holds the port, which AD-1 allows).
- *core author:* `App::shutdown` drains lanes, then iterates shards and writes
  `SessionStore` itself, because it "owns lifecycle".

The result is double writes, and the order can reverse: core's write can land with a
stale sequence after gateway's final write. Neither author knows who fires the
shard-status Lifecycle Hooks, since hooks are core descriptors but the gateway
observes the events. `--no-resume` clearing is unowned. Nothing says whether
interactions that arrive while shards are closing get answered (the HTTP client is
still up) or dropped.

**Proposed AD-21: The gateway owns session state; core owns the shutdown sequence.**
> - `noctis-gateway` is the only reader and writer of `SessionStore`. It persists `(session_id, seq, resume_url)` only after observing `GatewayClose` following a `CloseFrame::RESUME`, and clears entries in `--no-resume` mode.
> - `noctis-gateway` reports shard status as a `ShardEvent` enum on a channel it returns from `start`; `noctis-core` is the only crate that turns these into Lifecycle Hooks.
> - `noctis-core` owns the shutdown sequence, in this order: (1) fire `ShutdownStarted`; (2) call `gateway.close_all(Resumable|Fresh)`. Shards stop enqueuing, and interactions already enqueued remain; (3) drain both lanes until empty or the grace period ends, with Responder/HTTP still live; (4) await `gateway.closed()` (sessions persisted); (5) drop ports. A handler still running at the end of the grace period is cancelled and reported to the Error Sink.
> - The resumed sequence is the last sequence *received*, not *handled*. Events dropped by back-pressure are not replayed (at-most-once), and the docs say so.

### H6: Port key namespacing and "store ports"

AD-14 says "keys are namespaced `<app>:<port>:<key>`" and that typed access is a
wrapper in `noctis-core`. FR-39 says the **conformance suite** tests App namespacing.
- *Adapter author (Redis):* reads the conformance suite, sees that namespacing is
  tested, and prefixes keys in the adapter.
- *core author:* prefixes in the typed wrapper.

The result is `app1:ref:app1:ref:Xyz`, or neither prefixes and Apps collide. FR-71
also lets an Adapter opt *out* (shared CacheStore), and there is no API for that.
- `<app>` is undefined. It could be the application id snowflake, a configured name or
  a process-local counter, and the choice must be stable across restarts and Cluster
  processes.
- "Store ports operate on keys plus `Bytes`" can't express FR-46. "N uses per window",
  checked across a Cluster, needs an atomic increment-with-TTL. A get/put
  RateLimitStore is racy, and each adapter author will invent their own atomicity.
- `IdentifyQueue` (a lease or semaphore) and `ModuleStateStore` (which needs
  change propagation under FR-45) are not KV stores at all.

**Tighten AD-14, replacing the "Store ports" and "Keys" bullets with:**
> - Ports are of three kinds, each with its own trait shape and conformance suite: **KV** (`RefStore`, `SessionStore`, `CacheStore`, `RenderCache`, `AssetStore`, `DiscordCache`: `get`/`put(ttl)`/`delete`, plus `take` for RefStore); **Counter** (`RateLimitStore`: `hit(key, window, limit) -> Decision` atomic, `set_until`); **Coordination** (`IdentifyQueue`: `acquire(bucket) -> Lease`; `ModuleStateStore`: `get`, `set`, `version() -> u64` polled for propagation).
> - Namespacing is applied **only** by `noctis-core`'s `Namespaced<P>` wrapper. Adapters store keys verbatim and must not prefix. The conformance suite tests namespacing through the wrapper.
> - `<app>` is `AppId`, a configured `[a-z0-9-]{1,32}` string, defaulting to the application id snowflake in base62. It must be identical across all processes of a Cluster. An Adapter opted into sharing (FR-71) is wrapped with `Shared<P>`, which uses the prefix `_shared:<port>:`.
> - `<key>` segments are produced by a core key builder that escapes `:`. Adapters treat keys as opaque bytes.
> - Port values are `codec`-versioned envelopes (`schema: u8` + postcard). A schema mismatch reads as a miss, never as an error or panic.

---

## Major holes

### H7: Attachments have two naming authorities, no diff owner and no budget owner

- *ui author* (FR-30, "names generated, unique within the message"): `View` assigns
  `att0.png` and `att1.png` in render order.
- *image author* (FR-36, "attachment filename derived from the cache key"): returns
  `Attachment { name: "<hash>.webp" }`.

When the ui tracker renames image output, the cache-key filename is lost. When it
doesn't, ui uniqueness breaks if one image is placed twice. On edit, "keeps it
without re-uploading" needs the *existing* message's attachment ids. Only core sees
the interaction message. If ui uses ordinal names, a reordered View re-uploads
everything. AD-11 says the Responder "attaches tracked attachments", but the
**Upload Budget** comes from the interaction's `attachment_size_limit`, which only
core sees. Meanwhile the ui validator also claims "per-field limits … `attachment://`
references" (FR-24). That gives two places that could reject an oversize file, with
different budgets.

**Proposed AD-22: Attachments are content-addressed ui values; core diffs and budgets them.**
> - `noctis-ui` defines `Attachment { name, bytes: Bytes, content_type, alt: Option<String> }` with the only constructor `Attachment::new(bytes, ext)`, which sets `name = "<blake3(bytes)[..16] hex>.<ext>"`. `noctis-image` builds its outputs with this constructor; since FR-36 needs the name to follow from the cache key, image passes the cache key's digest through `Attachment::with_digest`, and no other naming exists.
> - A rendered View is `(MessageTree, AttachmentSet)`. The set is keyed by name, so one image used twice is uploaded once.
> - `noctis-ui` validates only references, meaning every `attachment://x` has a member of the set. `noctis-core`'s Responder alone enforces the Upload Budget and computes the edit diff: kept = names present in the interaction message's attachments (sent as `{id}` with no bytes), new = uploaded, absent = dropped.

### H8: Where async rendering happens relative to a sync View

`View` (ui) is "a function from state to a message", which makes it sync and pure.
Rendering is async and lives in `noctis-image`, which `noctis-core` doesn't depend on.
- *ui/image author A* adds `Image::template(T, data)` as a lazy node in the View tree.
  They expect the Responder to render it.
- *core author* can't call the Renderer, because the DAG has no `core → image` edge.

The lazy node never renders. FR-34 also expects "a Command that triggers a render
automatically uses FR-21", which means the render must happen *inside* the handler
future so that the auto-defer timer covers it.

**Add to AD-22:**
> - Views contain only ready `Attachment`s. Rendering is awaited in the handler (`ctx.render(template, data).await -> Attachment`), where `ctx.render` comes from a `Renderer` trait defined in `noctis-ports` (the FR-37 seam) and implemented by `noctis-image`. The App holds `Option<Arc<dyn Renderer>>`, and calling `render` without one returns a typed error.

### H9: Computing field widths between codec and macros

AD-6 says `#[derive(Payload)]` "computes the bound from field types". A proc macro
sees tokens, not types.
- *macros author* pattern-matches `u64 → 11` and `String → None`. A type alias,
  `Option<u64>` or a user type (FR-16 extension) is then unknown.
- *codec author* encodes `u64` as base62 with a sign or a length prefix, which needs
  12 characters, or escapes `BoundedStr<N>` to a worst case of 2N.

The macro bound and the real encoding disagree, and AD-3's "never copy a constant"
doesn't cover codec widths because they aren't rules.

**Tighten AD-6, adding:**
> - Every field type implements `noctis_codec::Field` with `const MAX_LEN: Option<usize>`, which includes its own worst-case escaping. `BoundedStr<N>::MAX_LEN = 2N`. `#[derive(Payload)]` never inspects type names. It emits a `const` expression summing `<F as Field>::MAX_LEN` plus `(fields-1)` separators, combined with `Option` arithmetic in a `const fn` exported by the codec.

### H10: What `<ver>` means

AD-5 calls `<ver>` "one base62 character, which lets the format evolve", which is a
**wire-format** version. The addendum (FR-60) says "the codec's version tag is per
Payload type", which is a **schema** version. One implementer bumps `<ver>` when a
Payload struct changes. Another treats `<ver>` as global and adds no schema field.
Stale detection then works on one side only.

**Tighten AD-5:**
> - `<ver>` is a single base62 character encoding `format_version` (high bits, currently 0) and the Payload's `#[payload(schema = n)]` (n < 62/k). If the two can't share one char, the format is `<fmt><schema><route>:…`, and the AD-6 bound counts both. A decode with an unknown `fmt` is `CodecError`. A known `fmt` with a different `schema` is `Stale` (FR-60), never `CodecError`.

### H11: i18n vs ui, macros and commands

1. **Chrome strings.** "Step 2 of 3: Roles", "Page 2/7", the Expired notice, the
   rate-limited notice and the "disabled here" notice are all framework text. The
   Paginator and Wizard live in `noctis-ui`, which can't depend on `noctis-i18n`.
   The ui author hard-codes English, while the i18n author ships `noctis.*` keys
   nobody calls.
2. **Locale type.** ui, core, i18n and testkit each need a `Locale`. With no owner,
   there will be three enums plus twilight's string.
3. **Command localization source.** AD-16 says "command localizations are read from
   the same files". `command!` (macros) can read locale files at compile time
   relative to *which* crate's `CARGO_MANIFEST_DIR`? FR-72 merges Module locale files
   at App build, and FR-41 needs localized names validated. One author validates in
   `command!` against the Module's own `locales/`. Another resolves keys at
   `App::build` from the merged set. They then disagree on key naming
   (`cmd.settings.logs.name` vs per-word `word.logs`).
4. **Overlays.** `LocaleSource` can overlay values at run time (AD-16). If it
   overlays a *command* name, Publish JSON and Discord diverge until the next
   Publish, and the overlay can bypass the name-regex check.

**Proposed AD-23: Localization ownership.**
> - `Locale` (Discord locale code enum + `Other(String)`) and `trait Translate { fn get(&self, key: &'static str, locale: &Locale, args: &[(&str, &dyn Display)]) -> Cow<str> }` live in `noctis-rules` (no deps). `noctis-ui` widgets take `&dyn Translate` and use only keys under the reserved `noctis.` namespace. `noctis-i18n` implements `Translate` and ships default `noctis.*` strings for `en`.
> - Command localization keys are fixed: `cmd.<path words joined by .>.name|desc`, `cmd.<path>.opt.<opt>.name|desc`, `cmd.<path>.opt.<opt>.choice.<value>`. Per-word path translation uses `cmdword.<word>`, with a full-path key overriding it.
> - `command!` validates only the default-locale names it can see in its own crate's `locales/`. Merged, cross-Module and non-default-locale validation (FR-41, FR-72) happens at `App::build` from the merged catalog. That is the single source for Publish JSON.
> - `LocaleSource` overlays apply to content keys only. Overlaying any `cmd.*`/`cmdword.*` key is rejected.

### H12: Order of the interaction pre-handler pipeline

Several behaviors run "before the handler": Route Guards (FR-19), rate limits (FR-46:
"before any auto-defer"), the module-disabled notice (FR-45), Middleware (FR-20, which
can override the locale, FR-43), Payload decode, stale/expired handling and auto-defer.
- *Author A* implements rate limits as built-in middleware registered first. The
  localized notice then uses the un-overridden locale, which violates FR-43.
- *Author B* runs rate limits after middleware. A slow middleware then consumes the
  2500 ms budget before the rate-limit check, and the auto-defer timer has already
  started.

Without a fixed order, Route-level overrides (FR-46 "overridden per Route") and Error
Sinks see different states.

**Proposed AD-24: Fixed interaction pipeline.**
> For every interaction, `noctis-core` runs exactly this sequence; each stage may end the interaction with an ephemeral reply via the Responder:
> 1. decode `custom_id` / resolve command (unknown or forged → FR-19 path);
> 2. resolve Ref (`take`/`get`) → missing → Expired Handler; schema mismatch → stale handler;
> 3. Boot-flag / App allow-list / Module-enabled check → "disabled here";
> 4. locale resolution (User → Guild → default);
> 5. before-Middleware, parent Module first (may override locale, attach Extensions, stop);
> 6. Route Guards;
> 7. rate-limit `hit`;
> 8. auto-defer timer armed (deadline measured from lane receipt, AD-10); handler runs;
> 9. after-Middleware (reverse order), then Error Sink routing.
> Stages 1–4 do no port I/O except the Ref lookup and the cached Module state. They also never call author code.

### H13: Middleware Extensions

FR-20 and AD-9 mention `Ext<T>` but don't define what happens when a parent and a child
Module both insert `T` (overwrite, or keep the first), or when a handler asks for an
`Ext<T>` that no middleware on its path provides. One implementer returns a runtime
`HandlerError`, while AD-4 (which wants App-build checks for anything that depends on
combining Modules) implies a build error. Those are two incompatible test suites.

**Add to AD-24 (or AD-9):**
> - Extensions are a per-interaction type map. `insert` of an existing `T` replaces it, so the child Module's value wins, and emits a `debug` trace. Values are `Send + Sync + 'static`.
> - Middleware descriptors declare `provides: &[TypeId]`, and handler descriptors declare the `Ext<T>` they extract. `App::build` fails if a handler requires `Ext<T>` that no before-Middleware on every ancestor path provides. `Option<Ext<T>>` opts out of the check.
> - Extensions are visible to after-Middleware and Error Sinks, and are dropped when the interaction ends. They are never written to a port.

### H14: Lane item shape and lane ownership

AD-10 binds both crates to "lanes" but doesn't say which crate defines them or what
an item carries. The addendum says Noctis parses raw shard `Message`s itself, because
twilight drops component types 21, 22 and 23. AD-12 says unknown data is kept as
`serde_json::Value`. FR-21 measures the deadline "from gateway receipt".
- *gateway author* enqueues `twilight_model::Interaction`, which has already lost the
  raw data and has no timestamp.
- *core author* expects raw JSON and an `Instant`.

The testkit then has to fake whichever shape won.

**Tighten AD-10, adding:**
> - `noctis-gateway` defines the lanes and the item type `Inbound { shard: ShardId, received_at: Instant, kind: Interaction(Box<Interaction>, RawJson) | Event(Box<Event>) }`. `RawJson` is the original `d` payload as `Bytes` and is kept only for interactions. `noctis-core` defines the worker set and drains the lanes. `noctis-testkit` injects `Inbound` values into the same lanes.
> - Per-guild ordered dispatch (FR-5) is implemented by core's worker set (guild-hash sub-queues), not by the gateway.

### H15: Who owns the HTTP client

The gateway needs HTTP for `/gateway/bot` (recommended shard count and
`max_concurrency`). Core needs it for responses and Publish. Image (FR-67) and
AssetStore need an HTTP client for CDN fetches. If the gateway builds its own
`twilight_http::Client`, a configured FR-52 proxy is bypassed for that call. That
violates "no request bypasses it" while each crate is locally compliant. The Layer
table says core may do I/O "through ports and twilight-http only", which gives the
gateway and image no rule at all.

**Proposed AD-25: One Discord HTTP client per App.**
> - `noctis-core` constructs the App's single `Arc<twilight_http::Client>` (proxy, timeout and rate limiter applied) and passes it to `noctis-gateway::start`. No other crate constructs a Discord HTTP client.
> - Non-Discord HTTP (CDN asset fetches) goes only through the `AssetStore` port's fetcher, which has its own client, allow-list and byte caps. It never goes through the Discord client.

### H16: Multiple Apps in one process

FR-47 runs several Apps in one process. AD-15's "dedicated rayon pool", takumi's
global glyph and parse caches ("set before first render") and `metrics` statics are
process-wide. If each App owns its render pool, N Apps get N pools, which breaks the
FR-34 bound and doubles memory. If one App owns it, the second App's builder
reconfigures the glyph cache after the first render, which is an unstable-API
failure. Metric names carry no App label, so two Apps' queue depths merge.

**Proposed AD-26: Process-scoped vs App-scoped resources.**
> - **Process-scoped** (one per process, built by a `noctis::Runtime` that Apps are attached to, or lazily on first App with config frozen at that point): render pool and its queue, takumi global caches, font registry, emoji font, metrics recorder.
> - **App-scoped:** HTTP client, gateway/shards, Registry, Responder, ports (wrapped per AD-14 namespacing), Theme, `State<T>`, Limits.
> - Every metric and every `noctis.*` tracing span carries an `app` label/field equal to `AppId`. Render cache keys include the Theme hash (H17), so sharing the RenderCache across Apps is safe.

### H17: AD-15's cache key is narrower than FR-36

AD-15 hashes `template_id, template_version, postcard(data), size, format`. FR-36
requires "settings, referenced asset versions, and Noctis and renderer versions". An
image author following AD-15 literally serves stale avatars and keeps serving
pre-upgrade images. The App Theme isn't in the key either, so white-label Apps that
share a RenderCache would show each other's colors.

**Tighten AD-15:**
> - The key is `blake3(KEY_SCHEMA, noctis_version, takumi_version, template_id, template_version, theme_digest, postcard(data), sorted(asset_ids_with_hash), size, format, encode_settings)`. Template data used in keys must serialize deterministically, so `HashMap` fields are forbidden in Template data, enforced by the derive.
> - Single-flight deduplication is process-local in `noctis-image`. The `RenderCache` port is not required to coordinate across processes.

---

## Minor holes

**H18: Threading the `Limits` instance.** AD-3 lets callers override `Limits` at run
time. If a Paginator author packs with `Limits::default()` and the Responder validates
with the App's lower override, packing produces pages that the validator then rejects.
*Tighten AD-3:* "Every ui function that measures, packs or validates takes
`&Limits` explicitly. `Limits::default()` is called only by `App` builder defaults,
macros and tests."

**H19: Rebuilding vs swapping.** AD-7 says enabling or disabling a Module "never
rebuilds the Registry … flips a per-Module bit", and also that "swapping the Registry
at run time uses `arc-swap`". The addendum says the dispatch index is swapped on
enable or disable. One implementer swaps, another checks bits, and their tests
differ. *Tighten AD-7:* "The Registry is immutable for the App's life. Module
enablement is a per-Module bitset (global + per-guild overrides via ModuleStateStore
cache) checked at pipeline stage 3. `arc-swap` holds only that bitset/cache snapshot."

**H20: Macro path roots.** The macros emit code that refers to runtime types. If the
generated code uses `::noctis::…`, any internal crate or test that uses `view!` or
`#[derive(Payload)]` without depending on the facade breaks, and the DAG forbids that
dependency. If it uses `::noctis_codec::…`, bot crates break, because AD-2 says they
depend only on `noctis`. *Tighten AD-2:* "Macro output refers only to `::noctis::__private::<crate>` paths. Internal crates that need derives in their own tests use a dev-dependency on `noctis` (permitted for tests) or the `#[noctis(crate = path)]` override."

**H21: The key type in `HandlerError`.** AD-13 says the error carries an "optional
localized user message key". `i18n!` generates typed functions, not keys. *Tighten
AD-13:* "`i18n!` also emits a `MessageKey` const per key (newtype over
`&'static str`, defined in `noctis-rules` alongside `Translate`), and `HandlerError` and
`<Area>Error::user_key()` return `Option<MessageKey>`."

**H22: Three kinds of rate limit vs AD-1.** twilight-http-ratelimiting keeps
process-local state, and per-shard gateway-command limits do too. AD-1 read literally
("anything that stores, caches, queues … must be a port") forces them through
`RateLimitStore`, which another implementer won't do. *Tighten AD-1:* "Exempt:
transport-level limiter state of twilight-http (superseded by the FR-52 proxy in
Clusters) and per-connection gateway state. `RateLimitStore` is used only for
author-declared FR-46 limits."

---

## Seams checked and found sound

- Command Pattern expansion and validation (AD-8) shares one `validate_tree` across
  the macro and App-build, so it holds.
- Codec totality and ASCII-only (AD-5) hold, once H9 and H10 are fixed.
- Error-type layering (AD-13) holds, apart from H21.
- `noctis-prefix → core` adds no shared entity beyond what AD-24's pipeline covers. It
  should reuse stages 3–9.
