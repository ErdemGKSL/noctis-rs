---
title: 'Input reconciliation: ARCHITECTURE-SPINE.md vs PRD addendum, rules catalog, brief addendum'
created: '2026-10-19'
spine: ../ARCHITECTURE-SPINE.md
inputs:
  - ../../../prds/prd-noctis-rs-2026-10-19/addendum.md
  - ../../../../../docs/discord-rules.md
  - ../../../briefs/brief-noctis-rs-2026-10-19/addendum.md
  - ../../../prds/prd-noctis-rs-2026-10-19/prd.md (consulted for UD-*, NFR-*, FR-8/16/17/60)
status: proposal (spine not edited)
---

# Input reconciliation

Findings are ordered by severity. **C-n** = contradiction (spine and an input disagree;
one must change). **G-n** = gap (an input requirement the spine does not carry).
Each item ends with proposed spine text. AD numbers 18+ are new.

---

## 1. Contradictions

### C-1 — `custom_id` length arithmetic is off by one between spine and catalog (blocking)

- Spine AD-5/AD-6: `custom_id = <ver><route>:<fields>`; by-ref length
  `ver + route + 1 + 12` = `route + 14`.
- Catalog CID-01 and Compile-time strategy §1: `custom_id = <route><SEP><payload>`;
  by-ref `route.len() + 13`; "Route literal ≤ 87 characters". No version char.
- Every trybuild boundary fixture ("100-char custom_id") will disagree with one side.

**Proposal:** the spine's version char stays (it is needed by FR-60, see C-4). Amend
the catalog (§1 formula, CID-01 "How" column, the ≤ 87 sentence) to:

```text
custom_id (inline) = <ver:1> <route> ':' <field> (',' <field>)*
custom_id (by ref) = <ver:1> <route> ':' '~' <11 base62>
max_len(by ref)    = 1 + route.len() + 1 + 1 + 11 = route.len() + 14   (≤ 100 ⇔ route ≤ 86)
max_len(inline)    = 1 + route.len() + 1 + Σ field widths + (fields − 1)
```

Add to AD-5: "The catalog's Compile-time strategy §1–2 formulas are normative and must
match this layout character for character; `xtask rules-catalog` asserts the
constants (`VER_LEN`, `SEP_LEN`, `REF_MARKER_LEN`, `REF_LEN`) quoted in the catalog
equal those in `noctis-codec`."

### C-2 — Route id charset and length

- Spine AD-5 and Conventions: `[a-z0-9-]{1,24}`.
- Catalog CID-03: `[a-z0-9_.-]+`, "length ≤ a configured budget"; all catalog
  examples use dots (`shop.buy`, `wizard.step`), and §1 reasons about routes up to 87.

**Proposal:** keep the spine's 24-char cap (it keeps inline budget large) but allow
`.` and `_` so tescord-style dotted names port directly:
"Route id: `[a-z0-9._-]{1,24}`, must not contain `:` `,` `~` `!`; checked at compile
time (CID-03)." Update CID-03 to drop "configured budget" (a runtime config cannot
gate a compile-time check) and to state the 24 cap; fix §1 to "route ≤ 24, so a
by-ref Route always fits (24 + 14 = 38)".

### C-3 — Event-lane back-pressure contradicts the addendum's read-loop rule

- Spine AD-10: "When [the event lane] is full, the shard task applies back-pressure."
- PRD addendum, Gateway and dispatch: shard tasks "never `send().await` into a bounded
  handler queue, which would stall the read loop, miss heartbeat ACKs and get the
  connection dropped … other events go to a bounded queue with **per-type drop
  policy**. Optional per-guild ordering by guild-hashed workers."

Back-pressure on the shard task is exactly the failure the addendum forbids.

**Proposal (replace AD-10 bullet 3):**
"Other events go to the bounded **event lane** with `try_send`. On full, the
event's `DropPolicy` applies (per event type, configurable: `DropNewest` default,
`DropOldest`, or `Block` only for types the author marks critical, which then use a
separate small critical lane). The shard task never awaits lane capacity. Drops are
counted in `noctis_events_dropped_total{type}`. Optional per-guild ordering hashes
`guild_id` to a fixed worker."

### C-4 — `<ver>` is a format version in the spine, a per-Payload schema version in the addendum

- Spine AD-5: `<ver>` "lets the format evolve" (one global codec version).
- PRD addendum + FR-60: "the codec's version tag is per Payload type; a mismatch
  routes to the Expired Handler or the Route's stale handler"; compatible changes
  (adding an optional field) must still decode.

**Proposal (AD-5 amendment):** "`<ver>` is the Payload's schema version, base62
`0..=61`, declared `#[payload(version = N)]` (default 0). Decode with an unknown or
newer version → `CodecError::Stale` → Route stale handler, else Expired Handler
(FR-18). An older version decodes when the derive is given `#[payload(compat = [..])]`
and the only change is trailing `Option<T>` fields (absent trailing fields decode as
`None`). A global codec format change is a major release, not a `<ver>` bump."

### C-5 — Responder: runtime state machine vs compile-time typestate

- Spine AD-11: Responder is a state machine; "a transition that isn't legal is an
  error" (runtime).
- Catalog INT-03/04/05/06/08 mark these **Compile-time**: `Responder<Pending>`
  methods consume `self` and return `Responder<Acked>`; per-kind contexts
  (`CommandCtx` has no `update()`, `ModalSubmitCtx` has no `modal()`,
  `AutocompleteCtx` has only `respond`), `defer()` takes only `Ephemerality`.
- Spine AD-4 itself says static mistakes must be compile-time, so AD-11 contradicts AD-4.

**Proposal (replace AD-11 rule bullets 1 and 4):**
"`Responder<S>` is a typestate: `Pending` → (`reply` | `defer` | `update` | `modal`)
consume `self` → `Acked`, which offers only `edit_original` and `followup`. Each
handler kind gets its own context type exposing only legal callback types (INT-04).
Auto-defer races the handler on the same state behind an atomic; when it wins, the
handler's `Responder<Pending>` call transparently becomes `edit_original` — this is
the only runtime transition check. A Route or Command that may open a modal declares
`opens_modal`, which disables auto-defer (INT-05). Default auto-defer deadline 2500 ms
from gateway receipt; commands defer with type 5 (flag `EPHEMERAL` only), components
and modal submits with type 6; the V2 flag is applied on edit-original (INT-08)."

### C-6 — `noctis-rules` dependency budget vs CMD-01

- Spine AD-3: rules crate's only non-std dependency is `unicode-ident` or equivalent.
- Catalog CMD-01: name regex `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$`,
  "compiled with `regex` + Unicode script tables inside the proc macro"; CMD-02/03
  need UTF-16 length and lowercase mapping. `unicode-ident` covers none of `\p{L}`,
  Deva/Thai script, or case.

**Proposal (AD-3 bullet 1):** "`noctis-rules` is sync, side-effect-free and
`no_std + alloc`-compatible; its non-std dependencies are limited to Unicode table
crates (`unicode-properties`/`unicode-script` or equivalent, no `regex`). CMD-01 is
implemented as a hand-written char-class check over those tables so the proc macro and
runtime share it."

### C-7 — AD-8 says "mixing leaves with groups" is checked; catalog says it is legal

- Catalog CMD-14: a command may hold groups and plain subcommands side by side
  (documented specifically "so the validator does not reject a valid tree").
- Spine AD-8 lists "mixing leaves with groups" among violations. The real rules are
  CMD-11 (same *name* both leaf and group) and CMD-12 (base with children is not a leaf).

**Proposal (AD-8 bullet 1):** replace "mixing leaves with groups" with "a name that is
both a leaf and a group at one level (CMD-11), a base that is both invokable and has
children (CMD-12), > 25 children per node (CMD-13), > 3 words (CMD-10), and the 8000
budget (CMD-05). Sibling groups and subcommands under one base are legal (CMD-14)."

### C-8 — Pattern expansion cap and order: PRD vs catalog (spine silent)

- PRD FR-8: cap configurable, **default 25**; combinations "left to right in
  declaration order".
- Catalog CMD-15: cap **64** **[N]**; "breadth-first over the first group".

**Proposal (AD-8 new bullet):** "Expansion order is left-to-right, declaration order
(FR-8). The cap is 25 expansions (FR-8 default; `#[command(max_expansions = N)]`
raises it to ≤ 64). Duplicate expansions are rejected (CMD-15)." Update CMD-15 to 25.

### C-9 — Render cache key omits upgrade-sensitive inputs

- Spine AD-15: `blake3(template_id, template_version, postcard(data), size, format)`.
- PRD addendum Render cache: key also covers **settings**, **referenced asset
  versions (e.g. avatar hash)**, **Noctis and takumi versions**.
- Spine's key serves stale images after an avatar change or a takumi upgrade.

**Proposal (AD-15 bullet 3):** "Key = `blake3(template_id, template_version,
postcard(data), size, format, render_settings, asset_versions, NOCTIS_VERSION,
TAKUMI_VERSION)`."

### C-10 — Payload width trait shape differs

- Spine AD-6: `Payload::MAX_ENCODED_LEN`, `#[derive(Payload)]` separate from
  `#[derive(Route)]`; route attr `overflow = "spill"` (string).
- Catalog §2: per-field `PayloadField::MAX_WIDTH`; `derive(Route)` placed directly on the
  payload struct with `#[route("shop.buy")]`; `Route::MAX_LEN`;
  `__assert_custom_id_fits`; `overflow = spill` (path); `#[route("wizard.step", by_ref)]`.

**Proposal (AD-6, normative names):** "`PayloadField { const MAX_WIDTH: Option<usize> }`
is implemented by every field type; `Payload { const MAX_ENCODED_LEN: Option<usize> }`
is derived as Σ widths + separators. `#[derive(Route)]` on a struct implies
`Payload` for it; `#[route("id", by_ref | overflow = spill)]` is the one attribute
syntax. The derive emits a free `const _: () = noctis::__assert_custom_id_fits::<T>();`
and rejects generic Routes (a post-monomorphization assert would only fire under
`cargo build`). It also emits a `#[test]` printing the width breakdown (const panics
can't format numbers)." Update catalog examples to the same syntax.

### C-11 — OPT-01 left to "architecture"; PRD already decided

- Catalog OPT-01: derive "rejects out-of-order fields or (default [N]) reorders them …
  Choice made in architecture."
- PRD FR-10: out-of-order required/optional "are compile errors".

**Proposal (convention row):** "OPT-01: `derive(Options)` rejects a required field after
an optional one (no silent reordering)." Fix the catalog's "(default) reorders".

### C-12 — FR-17 "return an error" overflow policy vs FR-16/AD-6 "never exceeds 100"

The PRD offers a per-Route policy "spill or return an error"; AD-6 makes non-fitting
Routes fail to compile unless `by_ref`/`spill`. Proposal: keep AD-6; state "the
`error` policy applies only to `BoundedStr` values whose runtime escaped width exceeds
the inline slot; unbounded fields cannot choose it." (Or drop it from FR-17.)

### C-13 — Minor pins

- takumi: spine "2.17.0 (pinned exact)"; addendum "2.17.x". Pick `=2.17.0` and note
  "bump deliberately, gated on pixel snapshots (NFR-8)".
- Memlog still says rule ids `DR-xxx`; spine correctly uses `<AREA>-<nn>`. Fix memlog.

---

## 2. Quiet requirements that did not land

### G-1 — Allocation discipline (UD-8, NFR-1, NFR-9; brief "memory allocation stays small … millions of users") — **no AD**

The spine never commits to the allocation budget; the memlog's rationale
("BoxFuture is one small alloc per call; hot paths never call ports") is not in the spine.

**Proposed AD-18 — Allocation and memory discipline**
- **Binds:** `noctis-codec`, `noctis-core`, `noctis-gateway`, `noctis-ports::memory`; UD-8, NFR-1, NFR-9.
- **Prevents:** per-interaction heap churn and unbounded framework memory at millions of users.
- **Rule:**
  - Route lookup and fixed-width Payload decode perform **zero heap allocations**:
    the codec decodes from `&str` slices of the incoming `custom_id`; the Registry maps
    are keyed by `&'static str` / interned ids and looked up with borrowed keys; string
    Payload fields decode to `Cow<'a, str>` (borrowed unless unescaping is needed).
  - Encoding writes into a stack `ArrayString<100>` (or equivalent), never a `String`
    built by concatenation.
  - Port calls (one `BoxFuture` each) are allowed only off the inline-payload path;
    a by-ref decode is the only dispatch step that may call a port.
  - Every framework cache and queue is bounded by a byte or entry budget set in config
    with a documented default (moka weigher by bytes). There is no unbounded `HashMap`
    keyed by guild, user, message or interaction.
  - Per-guild framework state is a fixed-size record (module-enable bitset, locale);
    budget ≤ 1 KiB/guild excluding the Discord cache (NFR-1).
  - CI runs a counting-allocator benchmark (`dhat` or a `GlobalAlloc` counter) on the
    dispatch path and fails on any allocation in Route lookup or fixed-width decode,
    and on regressions beyond the documented budget elsewhere.

### G-2 — tescord-like ergonomics and tone — **absent from the spine**

Brief: "follow tescord as closely as practical; features matter more than identical
output"; PRD Vision: "feel as direct as tescord … Rust's type system should catch the
mistakes tescord only finds at runtime"; UJ-4 migration; Pack → Module concept map.

**Proposed AD-19 — Authoring surface mirrors tescord**
- **Binds:** `noctis-macros`, `noctis-core`, `noctis` facade, Usage Skill; FR-7/8/13, UJ-4.
- **Prevents:** an API that is idiomatic-but-foreign to the target author, making migration a rewrite.
- **Rule:**
  - A Command is declared by its space-separated Command Path or Pattern on an
    `async fn`: `#[command("settings logs channel")]`, `#[command("config (set|get) (prefix|welcome)")]`.
    Word count decides command / subcommand / group (CMD-10). No builder trees for
    static commands.
  - A Module is the unit a tescord Pack maps to: `Module::new("a/b")` collecting
    commands, Routes, events and Middleware; Modules merge by base word (FR-9).
  - Components are built from Route handles (`BuyButton { .. }.button()`), mirroring
    tescord's `buildComponent`, with data after the base id (UD-7).
  - Publish is "auto" in the tescord sense — the author writes no registration code —
    but it is diff-based and runs only in the publisher process or CLI mode (FR-13),
    never on shard ready.
  - Naming follows tescord where a concept exists (Module ≈ Pack, Route, Ref,
    Locale); the Usage Skill ships a tescord → Noctis concept map.
  - Where tescord's runtime behaviour and Discord's rules disagree, Noctis follows
    Discord and fails at compile time (e.g. no `"..."` description placeholder, CMD-04).
- Tone convention: errors and docs are short, imperative, and name the fix
  (see G-6).

### G-3 — UI doctrine (UD-1..UD-8) bound in front-matter but not architected

No AD says how Views/Navbar/Wizard/Paginator realize UD-1..6, or that built-ins never
emit `content`/embeds.

**Proposed AD-20 — UI doctrine is built into `noctis-ui`**
- Built-in widgets (`View`, `Navbar`, `Wizard`, `Paginator`) only produce `V2Message`;
  legacy `content`/embeds exist only behind `LegacyMessage` (CV2-02/15).
- `Navbar` is a top action row of Route buttons whose Payload is the tab index; it swaps
  the Container content in place (`UPDATE_MESSAGE`). `Wizard` carries step index +
  state in Payload/Ref and renders a progress indicator, never "page n of m" (UD-4).
- `Paginator` paginates automatically when a homogeneous list would exceed CV2-03 (40)
  or CV2-04 (4000) after validation; inside a Navbar tab when present (UD-6). The page
  indicator is a disabled button with a unique `custom_id` (CID-02).
- Views default to `allowed_mentions.parse = []` (MSG-05, NFR-6).
- Images (`noctis-image`) are attached into Views (Thumbnail/Media Gallery) and never
  carry actionable information (UD-2).
- Each node type has `const COST: usize` and `const TEXT: Option<usize>`; static
  `view!` trees get `const { assert!(COST <= 40) }` and the literal text sum ≤ 4000;
  dynamic children contribute a static lower bound plus a runtime check (catalog §5).

### G-4 — Compile-time strategy specifics not in the spine

Add to AD-4 / AD-6 / AD-12 (or a new "Compile-time toolkit" convention block):
- **Payload field widths (normative table, from catalog §2):**

  | Type | `MAX_WIDTH` |
  | --- | --- |
  | `bool` | 1 |
  | `u8` | 2 |
  | `u16` | 3 |
  | `u32` | 6 |
  | `u64`, `Id<T>` | 11 |
  | `i8/i16/i32/i64` | unsigned width of the zig-zag value (2/3/6/11) — **not in catalog; propose** |
  | `#[derive(PayloadEnum)]`, ≤ 62 variants | 1 |
  | `Option<T>` | `T + 1` (presence tag) |
  | `BoundedStr<N>` | `2·N` (each char may escape to `!x`) |
  | `String`, `Vec<T>` | `None` |

  plus separators (`fields − 1`) and, when enabled, the **select nonce** (FR-25) and the
  **integrity tag** (FR-19) — both missing from AD-6's sum. Proposal: "`select` Routes
  reserve `NONCE_LEN = 2`; Routes with `#[route(signed)]` reserve `TAG_LEN = 6` (truncated
  HMAC, base62); both are included in `MAX_LEN`." Specify `BoundedStr` escaping so
  `2·N` is exact: non-`[A-Za-z0-9._-]` chars encode as `!` + one base62 code for an
  allow-listed ASCII set; non-ASCII is rejected at encode (`CodecError`), not escaped.
- **Ref constants:** `REF_LEN = 11` is a `pub const` in `noctis-codec`; Refs come from
  a CSPRNG (`getrandom`), never sequential or interaction-derived (CID-05); RefStore key
  `<app>:ref:<ref>`; per-Route TTL and `one_shot` (FR-17).
- **Literal newtypes:** public builders take `Label`, `Description`, `Placeholder`,
  `Title` etc., not `&str`; `X::lit("…")` is a `const fn` with an assert, macros always
  wrap literals in `const { … }`; runtime path is `X::try_from(String)`, same limit
  (catalog §4).
- **Arity via const generics:** `ActionRow::buttons::<N>` (1..=5), `Section::new([Text; N], acc)`
  (1..=3), `MediaGallery::new([Item; N])` (1..=10), each with `const { assert!(…) }`.
- **CMD-05:** each `#[command]` emits `const SIZE: usize`; App-build recomputes on the
  merged tree with longest-localization-per-field.
- **Missing macros in the seed:** `derive(Choices)`, `derive(PayloadEnum)`,
  `#[user_command]`, `#[message_command]`, `modal!`. Add to `noctis-macros` line.
- **Context-menu commands** (CTX-01/02): separate macros with no description/options.
- **Typed flag sets and mention enums** (INT-09, MSG-03, MSG-07, WH-02):
  `CreateFlags`, `EditFlags`, `CallbackFlags`, `Users::All | Users::Only(ids)`,
  `ForeignWebhook::send(StaticView)`. Add to AD-12.
- **Static CID-02:** `view!` rejects duplicate `(Route, literal Payload)`; runtime
  validator hashes all ids before send.
- **CV2-14:** numeric component ids assigned deterministically from the tree.

### G-5 — Character counting unit (catalog "Unconfirmed values") has no convention

**Convention row:** "String limits: framework-encoded strings are ASCII, 1 char = 1 byte.
For user text, `noctis-rules::len::discord_len(s) = max(scalar values, UTF-16 units)`
is the only length function; macros and runtime both call it."

### G-6 — Error message shape and rule-test layout

Catalog §3 and §6 fix a format the spine only gestures at ("every test names its rule id").

**Convention rows:**
- "Compile errors: `[<RULE-ID>] <what is wrong>` with `= note:` lines showing the
  derivation and a `= help:` line with a concrete fix; span on the offending word/field."
- "`BuildError` lists every violation; each names the rule id, the Module(s) involved and
  the actual vs allowed value. `--publish --dry-run` runs the same checks."
- "trybuild fixtures live at `tests/ui/<area>/<RULE-ID>_<slug>.rs` (+ `.stderr` containing
  the id) and boundary passes in `tests/ui-pass/`; UI tests run only on the pinned MSRV
  toolchain in CI. `xtask rules-catalog` fails when a Compile-time or App-build row has no
  test referencing its id."

### G-7 — Strict/lenient validation mode (FR-24) and the autocomplete exception

Spine never mentions it. **AD-3 addition:** "`Limits` carries `mode: Strict | Lenient`
(default Strict). Lenient truncates text with `…` and logs a warning; it never applies
to structural rules (CV2-03/05/06, CID-01/02). Autocomplete choices are always truncated
to 25 with a warning (FR-10 exception, INT-07)."

### G-8 — Startup safety and gateway specifics (addendum) — **no AD**

**Proposed AD-21 — Startup and gateway safety**
- `App::run` checks for a tokio multi-thread runtime and a rustls crypto provider,
  installs one if none, and returns typed errors instead of twilight's panics.
- Identify queue built from `GET /gateway/bot` `max_concurrency`, never
  `InMemoryQueue::default()` (GW-01); boot refuses when `session_start_limit.remaining`
  is too low (GW-02); Cluster Shard Ranges must be consecutive and aligned to
  `max_concurrency` (App-build).
- A Cluster config without a shared HTTP proxy is a `BuildError` (RL-02); the client is
  `ClientBuilder::proxy(url, true).ratelimiter(None)`; request timeout default 10 s.
  Rationale: twilight 0.17 retries 429s immediately and forever.
- Sessions store session id, sequence **and** `resume_url`; Noctis closes with
  `CloseFrame::RESUME` and polls until `GatewayClose` to save the final sequence.
- Noctis owns the Intents → `EventTypeFlags` → cache `ResourceType` mapping; required
  intents are derived from registered handlers and a missing one is a `BuildError`
  (GW-05). Interaction-only default: `Intents::GUILDS`.
- Shard tasks consume raw gateway `Message`s and call `twilight_gateway::parse`
  themselves so component types 21/22/23 and unknown types reach `noctis-ui` as raw
  JSON (supports AD-12).
- Gateway commands go through `MessageSender`, 120/60 s per shard (GW-04);
  `Event::RateLimited` is surfaced.

### G-9 — Rendering specifics (addendum) missing from AD-15

Add to AD-15:
- Animated default WebP, fallback GIF; degrade order fps → viewport → lossy quality →
  still frame toward a ~2 MB target, capped by the interaction's
  `attachment_size_limit` (default 20 MiB, ATT-01).
- Per-job caps (configurable): ≤ 4 MP per job, ≤ 1 MP per frame, ≤ 90 frames, ≤ 6 s;
  FPS caps GIF 50 / WebP-APNG 90. takumi can't be interrupted mid-frame, so caps are
  enforced before submission.
- Global caches set before first render: glyph 8 MiB, Tailwind parse 1 MiB, resource
  64 MiB per handle (counts toward NFR-1's separately reported render budget).
- Theme = stylesheet with `@theme` tokens and `@keyframes` passed via render options;
  redefining a built-in animation name (`spin`) is a registration error (FR-69).
- Tailwind **v4** class names only (validated by takumi's parser).
- No bundled font; emoji need a COLR font behind `image-emoji`; custom emoji fetched as
  images via `AssetStore` with a host allow-list (NFR-6).
- `image` feature off by default (≈100 crates, `libwebp-sys` needs a C compiler); takumi
  types exposed only under `noctis::image::unstable`.

### G-10 — Smaller items

- **Webhooks (WH-01..03):** always `?with_components=true`; per-endpoint flag sets. Add to AD-11/AD-12.
- **Interaction token** (INT-02): Responder stores receive time and returns
  `TokenExpired` without an HTTP call after 15 min; follow-up cap 5 for user-installed
  apps (INT-11). Add to AD-11.
- **Locales:** files named by Discord locale code (`tr.yaml`), closed `Locale` enum of 32
  (LOC-01); `LocaleSource` overlays are re-validated against CMD-01..05/OPT-09 at App-build
  (LOC-02). Add to AD-16.
- **`Limits` and macros:** AD-3 says macros use defaults; add "compile-time defaults are
  the conservative values; a runtime override that raises a [U] limit does not relax
  compile-time checks, one that lowers it is caught by the runtime validator."
- **Adapter principle breadth (brief):** AD-1 should enumerate the v1 port list so none
  are forgotten: `RefStore`, `SessionStore`, `IdentifyQueue`, `ModuleStateStore`,
  `RenderCache`, `AssetStore`, `LocaleSource`, `RateLimitStore` (cooldown buckets),
  `Cache` (Discord cache backend). RefStore methods `put/get/delete` + optional `touch`.
- **twilight 0.17.1 media `content_type: null` bug** is the concrete reason Noctis
  serializes its own media items with unset fields omitted; add to AD-12 "Prevents".

---

## 3. Suggested follow-ups to the inputs (not the spine)

- `docs/discord-rules.md`: apply C-1 (formula + ≤ 86), C-2 (CID-03), C-8 (CMD-15 = 25),
  C-10 (attribute syntax/names), C-11 (OPT-01 reject), and add signed-int and nonce/tag
  rows to §2.
- `.memlog.md`: replace `DR-xxx` with `<AREA>-<nn>`.
