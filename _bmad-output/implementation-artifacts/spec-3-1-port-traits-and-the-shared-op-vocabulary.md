---
title: 'Story 3.1: Port traits and the shared op vocabulary'
type: 'feature'
created: '2026-10-19'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-ports` is an empty skeleton. Later epics (Refs, sessions, Modules, rate limits, rendering, i18n, clusters) need one uniform, swappable storage layer, plus a render contract that core can call without depending on `noctis-image`. Bot authors need the same traits to write Redis or SQL Adapters (FR-38, NFR-6; AD-1, AD-14, AD-22).

**Approach:** Define in `noctis-ports`:
- the 11 dyn-compatible port traits, `Renderer`, `PortError` and the `BoxFuture` alias;
- the typed-value helpers (postcard plus a leading schema byte);
- the AD-22 render contract types.

Re-export all of it as `noctis::ports`, and `Bytes` as `noctis::bytes::Bytes`. Document the op semantics in a new Adapters reference of the Usage Skill. Bless the API snapshot. This story adds no memory defaults (3.2) and no IdentifyQueue implementation (3.3).

## Boundaries & Constraints

**Always:**

Async methods and port traits:
- Async methods return `BoxFuture<'a, Result<T, PortError>>`.
- `noctis_ports::BoxFuture<'a, T>` is `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;`. That is the same definition as `futures_util::future::BoxFuture`, so the two are the same type.
- Every port trait is `Send + Sync + 'static` (supertraits), has no generic methods, no `Self: Sized` methods and no associated consts/types, so `Arc<dyn Port>` works.

Keys and values:
- Keys are `&'a str`, values are `bytes::Bytes`, and TTL is `Option<Duration>` (`None` = no expiry).
- No method takes a token, a payload key or any secret (NFR-6).

Signatures (all methods borrow `&'a self` and `key: &'a str`; return types shown are the `Result` inside the `BoxFuture`):

| Op | Signature tail | Contract (rustdoc on each trait method, plus a crate-doc table) |
|---|---|---|
| `get` | `-> Result<Option<Bytes>>` | An expired or missing entry → `None`. |
| `put` | `(value: Bytes, ttl: Option<Duration>) -> Result<()>` | Overwrites the value and resets the TTL. |
| `put_if_absent` | `(value, ttl) -> Result<bool>` | `true` only for the one caller that inserted. An expired entry counts as absent. |
| `take` | `-> Result<Option<Bytes>>` | Atomic get+delete. Exactly one concurrent caller gets `Some`. |
| `delete` | `-> Result<()>` | Removes the entry. A missing key is not an error. |
| `touch` | `(ttl: Duration) -> Result<bool>` | `true` if a live entry existed; its expiry becomes now+ttl. A missing or expired entry → `false`, and nothing is created. |
| `incr_with_ttl` | `(by: u64, ttl: Option<Duration>) -> Result<u64>` | Atomic, no lost increments, and returns the new value. The TTL is set only when the call creates the entry (`by = 0` creates 0). The stored value is an 8-byte big-endian `u64`. The add saturates at `u64::MAX`. A non-counter value → `PortError::InvalidValue`. |

Port subsets (exactly these methods, no more):
- `RefStore`: put_if_absent, get, take, delete, touch.
- `SessionStore`: get, put, delete.
- `ModuleStateStore`: get, put, plus `version(&self) -> BoxFuture<'_, Result<u64, PortError>>`. The version is monotonic, starts at 0 on an empty store and grows on every `put`.
- `RateLimitStore`: incr_with_ttl, get, put.
- `CacheStore`: get, put, put_if_absent, delete, incr_with_ttl. These signatures equal the ones in `examples/project-template/src/adapters/bounded_cache.rs`, so that file compiles unchanged against them.
- `RenderCache` and `AssetStore`: get, put.

The other ports:
- `IdentifyQueue`: `acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>>`.
- `Lease`:
  - it is `#[must_use]`, `Debug + PartialEq + Eq`, and **not** `Clone`;
  - `Lease::new(bucket: u32)` is a const fn, and `bucket() -> u32`;
  - its docs say a Lease grants one identify in its bucket, and that a Cluster implementation serializes per bucket across processes.
- `LocaleSource`: `load<'a>(&'a self, locale: &'a str) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>>`. `locale` is a Discord code such as `"en-US"`. Each pair is (message key, value), and an empty Vec means no overlay.
- `GuildAccessList`: synchronous `allows(&self, guild: Id<GuildMarker>) -> bool`. It runs on AD-23 stage 3, which does no port I/O. Its docs say: never block; a remote-backed implementation keeps a local snapshot.
- `DiscordCache` uses only `twilight_model` types:
  - `update<'a>(&'a self, event: &'a twilight_model::gateway::event::Event) -> BoxFuture<'a, Result<(), PortError>>`.
  - Owned getters, each returning `BoxFuture<'_, Result<Option<T>, PortError>>`:
    - `guild(Id<GuildMarker>) -> Guild`
    - `channel(Id<ChannelMarker>) -> Channel`
    - `role(Id<RoleMarker>) -> Role`
    - `member(Id<GuildMarker>, Id<UserMarker>) -> Member`
    - `user(Id<UserMarker>) -> User`
    - `message(Id<MessageMarker>) -> Message`
  - Per-guild lists: `channel_ids`, `role_ids` and `member_ids(Id<GuildMarker>) -> Option<Vec<Id<…>>>`. `None` means the guild is not cached.
  - No reference, guard or lifetime-bound value is returned.
- `Renderer` (driven port, not a store):
  - `render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>>`;
  - `register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError>`.

Errors:
- `PortError` is a `thiserror` enum marked `#[non_exhaustive]`. Its variants:
  - `Backend(#[source] Box<dyn Error + Send + Sync>)`;
  - `InvalidValue { key: String, why: Cow<'static, str> }`;
  - `Unavailable { why: Cow<'static, str> }`;
  - `Value(#[from] ValueError)`.
- `PortError` constructors:
  - `backend(err: impl Into<Box<dyn Error + Send + Sync>>)`, which must work as `map_err(PortError::backend)`;
  - `invalid_value(key: &str, why: impl Into<Cow<'static, str>>)`;
  - `unavailable(why)`.
- `ValueError` is a `#[non_exhaustive]` enum with `Copy` and `Eq`. Its variants are `Serialize`, `Empty`, `Malformed` and `Trailing { extra: usize }`. It never wraps `postcard::Error`, so postcard stays out of the public API.

Typed values and the render contract:
- Typed values:
  - `encode_value<T: Serialize + ?Sized>(schema: u8, value: &T) -> Result<Bytes, ValueError>` produces `[schema] ++ postcard(value)`.
  - `decode_value<T: DeserializeOwned>(schema: u8, raw: &[u8]) -> Result<Option<T>, ValueError>` reads that format back. A different schema byte gives `Ok(None)` (a miss).
- Render contract, exactly as AD-22 lists it, in `src/render.rs`:
  - `Template` (consts `ID`, `VERSION: u32`, `SIZE: Size`, `FORMAT: Format`, `ANIMATION: Option<AnimationSpec>`, and `fn view() -> ErasedView`);
  - `ErasedView`, `TemplateDescriptor { id, version, view }`;
  - `RenderJob { template_id: &'static str, template_version: u32, data: Bytes, size, format, animation }`, where `data` is plain postcard with no schema byte;
  - `Size`;
  - `Format::{Png, Jpeg{quality: u8}, Webp{quality: u8}, WebpLossless}`;
  - `AnimationSpec { duration: Duration, fps: u16, format: AnimatedFormat }` with a const `new`;
  - `AnimatedFormat::{Webp{quality: u8}, Gif, Apng}`;
  - `Frame { index: u32, count: u32 }` and `Frame::STILL`;
  - `OutputFormat::{Png, Jpeg, Webp, Gif, Apng}` with `extension()`;
  - `RenderOutput { bytes, digest: [u8; 32], format: OutputFormat, settings_used: EncodeSettings }`;
  - `EncodeSettings { size, quality: Option<u8>, fps: Option<u16>, frames: u32 }`;
  - `RenderError`.
- Render enums are closed (not `non_exhaustive`). Render structs keep public fields so that `noctis-image` can build them. The exception is `Size`, which has private fields and a validating constructor.
- `noctis-ports` normal dependencies are exactly `bytes`, `postcard`, `serde`, `thiserror` and `twilight-model`. It has no workspace edge, and its `src/` does no I/O and uses no `unsafe`.
- Public API changes update `examples/project-template/` (the skill and its hash) in the same commit (AGENTS.md).

**Never:**
- No memory defaults, `noctis_ports::memory`, `Namespaced`/`Shared`, conformance suites, App builder setters or `ctx.kv()`/`ctx.cache()`: those belong to Stories 3.2, 3.3, 7.x, 8.x and 9.x.
- No `futures-util`, `tokio`, `twilight-cache-inmemory` or `moka` dependency in noctis-ports. No dev-dependency on tokio: tests poll ready futures with `std::task::Waker::noop()`.
- No dependency on `noctis-rules`, `noctis-codec` or any other workspace crate: the edge is not drawn, and deps-check fails.
- No serde derives on the render types, no `Template` derive and no `noctis::image` module (Epic 10). No re-export of `noctis::twilight` (later story).
- No new rule id or catalog change. No change to `docs/discord-rules.md`.

## I/O & Edge-Case Matrix

`encode_value` / `decode_value` (unit tests in `src/value.rs`; the sample type is `struct V { n: u32, s: String, v: Vec<u8> }`):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Round trip | `encode_value(7, &V{n:42,s:"a",v:[1,2]})` then `decode_value::<V>(7, ..)` | `Ok(Some(v))` equal to the input; byte 0 is `7` | — |
| Schema 0 / 255 | schemas 0 and 255 | round trip Ok; byte 0 equals the schema | — |
| Schema mismatch | encoded with 3, decoded with 4 | `Ok(None)` (miss) | none |
| Empty input | `decode_value::<V>(1, b"")` | `Err(ValueError::Empty)` | — |
| Schema byte only, type needs fields | `[1]` as `V` | `Err(ValueError::Malformed)` | — |
| Schema byte only, unit type | `encode_value(1, &())` == `[1]`; decode `()` | `Ok(Some(()))` | — |
| Trailing bytes | valid encoding + `[0, 0]` | `Err(ValueError::Trailing { extra: 2 })` | — |
| Garbage body | `[1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]` as `V` | `Err(ValueError::Malformed)` | — |
| Unicode | `s = "ğüş 😀 é\u{0}"` | round trip equal | — |
| Large value | `v` of 1 MiB | round trip equal; length = 1 + postcard length | — |
| Unknown-length seq | a type whose `Serialize` calls `serialize_seq(None)` | `Err(ValueError::Serialize)` | — |
| Error conversion | `PortError::from(ValueError::Empty)` | `PortError::Value(ValueError::Empty)`; `Display` is non-empty | — |

Render contract (unit tests in `src/render.rs`):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Template round trip | sample `Card { name: "Ada 😀", level: u32::MAX, tags: vec!["a",""] }` implementing `Template`; `RenderJob::of(&card)` then `decode_template::<Card>(&job.data)` | equal `Card`; job fields equal `Card::ID/VERSION/SIZE/FORMAT/ANIMATION`; `data` == `postcard::to_allocvec(&card)` (no schema byte) | — |
| Template data trailing | `job.data` + `[0]` | `Err(ValueError::Trailing { extra: 1 })` | — |
| Template data empty | `decode_template::<Card>(b"")` | `Err(ValueError::Malformed)` | — |
| Zero-field Template | `struct Blank;` (unit struct) | `RenderJob::of(&Blank)?.data` is empty; `decode_template::<Blank>(b"")` is Ok | — |
| Size bounds | `Size::new(1,1)`, `Size::new(u32::MAX,u32::MAX)` | `width()`/`height()` echo the inputs | — |
| Size zero | `Size::try_new(0,5)`, `(5,0)`, `(0,0)` | `None` | `Size::new` with a zero side panics (`#[should_panic]`); a const use is a compile error |
| Frame::STILL | — | `Frame { index: 0, count: 1 }` | — |
| Extensions | each `OutputFormat` | Png→`"png"`, Jpeg→`"jpg"`, Webp→`"webp"`, Gif→`"gif"`, Apng→`"png"` | — |
| Format → OutputFormat | `From<Format>`: Png→Png, Jpeg{..}→Jpeg, Webp{..}/WebpLossless→Webp; `From<AnimatedFormat>`: Webp{..}→Webp, Gif→Gif, Apng→Apng | as listed | — |
| AnimationSpec::new | `(Duration::from_secs(2), 20, AnimatedFormat::Gif)` in a `const` | fields echo the inputs | — |
| ErasedView | `ErasedView::new(5u8)` | `downcast_ref::<u8>() == Some(&5)`; `downcast_ref::<u16>() == None`; a clone shares the value; `Debug` prints no contents | — |
| TemplateDescriptor::of | `TemplateDescriptor::of::<Card>()` | `id == Card::ID`, `version == Card::VERSION`, and `view` downcasts to the marker the sample `view()` stored | — |
| RenderError::failed | `RenderError::failed(std::fmt::Error)` (no `std::io` in `src/`, per the purity test) | `source()` is `Some`; `Display` non-empty | — |

Ports (integration test `crates/noctis-ports/tests/ports.rs`):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Dyn compatibility | `Arc<dyn P>` for each of the 11 ports + `Renderer`; a `fn assert_port<T: ?Sized + Send + Sync + 'static>()` call per trait | compiles | — |
| Swap without caller change | a toy `CacheStore` (one `Mutex<HashMap>`) and one caller fn that takes `&dyn CacheStore` | put → get gives `Some`; `put_if_absent` on an existing key gives `false`; `delete` then `get` gives `None`; `incr_with_ttl(k,5,None)` twice gives 5 then 10 | — |
| Every signature callable | a stub for each of the other ports (returning `Ok(None)`/`Ok(())`/`Lease::new(b)`/`vec![]`/`true`), called through `Arc<dyn _>` with a polled future | the documented return types | — |
| Lease | `Lease::new(0)`, `Lease::new(u32::MAX)` | `bucket()` echoes | — |
| PortError constructors | `backend(io::Error::other("x"))`, `backend("text")`, `invalid_value("k", "why")`, `unavailable(String::from("down"))` | variants match; `source()` is `Some` for `Backend`; the `InvalidValue` Display contains `k` and `why` | used as `map_err(PortError::backend)` in the test |

</intent-contract>

## Code Map

Ports crate and workspace:
- `crates/noctis-ports/Cargo.toml` -- has no deps yet. Add `bytes`, `postcard` (features `["alloc"]`), `serde`, `thiserror` and `twilight-model`, all `.workspace = true`.
- `crates/noctis-ports/src/lib.rs` -- 4-line crate doc. Becomes the crate doc with an op-vocabulary table, the module declarations, flat `pub use` of every public item, and the `BoxFuture` alias.
- `Cargo.toml` (root) l.48 `postcard = "1.1"` -- unused so far (not in `Cargo.lock`). The defaults pull `heapless-cas` (heapless 0.7 → atomic-polyfill, a cargo-deny risk), so change it to `postcard = { version = "1.1", default-features = false }`. Verified on crates.io: postcard 1.1.3 (latest, MIT OR Apache-2.0); its features `alloc` and `use-std`; its only non-optional deps are `cobs ^0.3` and `serde`. Already locked: bytes 1.12.1, serde 1.0.229, thiserror 2.0.21, twilight-model 0.17.1 (latest).
- twilight-model 0.17.1 paths (checked in the registry source):
  - `twilight_model::gateway::event::Event` (enum, l.27 of `gateway/event/mod.rs`);
  - `guild::{Guild, Member, Role}`, `channel::{Channel, Message}`, `user::User`;
  - `id::{Id, marker::*}`.
  - `Member.user: User` is owned.

Facade:
- `crates/noctis/Cargo.toml` -- add `noctis-ports.workspace = true` and `bytes.workspace = true` (facade → any crate is allowed by deps-check).
- `crates/noctis/src/lib.rs`:
  - the `# Modules` list at l.6-14 gains `ports` and `bytes`;
  - follow the `pub mod codec` pattern (l.83-149): module doc plus an explicit `pub use noctis_ports::{…}` list; never a glob.

Tooling:
- `xtask/src/api_snapshot.rs` -- inlines internal re-exports. An external re-export (`noctis::bytes::Bytes`) stays a `pub use` line (module doc l.18-22). This is the first one, so check that its output is sane.
- `xtask/src/skill_check/mention.rs` -- every snapshot item's last path segment must appear as a whole identifier in the skill's `*.md`, including fields, variants, consts and methods (not trait-impl members).
- `xtask/src/skill_check/pending.rs` l.27-50 -- type names such as `File`/`Section`/`Label` outside `rules::`/`codec::` make `component-entries` due. No new type may use any of those names (none planned does).

Project template:
- `examples/project-template/.claude/skills/noctis/references/adapters.md` -- the existing planned Adapters reference: a port table, the CacheStore shape, a Redis sketch and conformance. Update it.
- `examples/project-template/.claude/skills/noctis/SKILL.md` l.18 -- the `**API hash:**` span.
- `examples/project-template/src/adapters/bounded_cache.rs` -- already written against exactly the CacheStore/PortError signatures above. It needs no edit; its planned-API NOTE stays because `noctis::testkit` has not landed yet.

Precedent and checks:
- `crates/noctis-rules/tests/purity.rs` -- the precedent for the purity test (forbidden tokens, dependency allow-list).
- `.github/workflows-disabled/ci.yml` -- no new job. The existing test/clippy/api-snapshot/skill-check jobs cover this story.

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` -- turn off postcard's default features (see the Code Map) -- avoid heapless/atomic-polyfill.
- `crates/noctis-ports/Cargo.toml` -- the 5 deps -- AC1.
- `crates/noctis-ports/src/error.rs` -- `PortError` plus its constructors -- AD-13 typed errors.
- `crates/noctis-ports/src/value.rs` -- `ValueError`, `encode_value` and `decode_value` (use `postcard::take_from_bytes` so trailing bytes are detected), with the unit tests from the value matrix -- AC3.
- `crates/noctis-ports/src/store.rs` -- the 7 byte-store traits with per-method contract docs -- AC1.
- `crates/noctis-ports/src/identify.rs` -- `IdentifyQueue` and `Lease`.
- `crates/noctis-ports/src/discord_cache.rs` -- `DiscordCache`.
- `crates/noctis-ports/src/locale.rs` -- `LocaleSource`.
- `crates/noctis-ports/src/access.rs` -- `GuildAccessList`.
- `crates/noctis-ports/src/render.rs`:
  - the render contract, `RenderError` (a `thiserror` enum marked `#[non_exhaustive]`: `UnknownTemplate { id: String, version: u32 }`, `Unsupported { id: String, why: Cow<'static, str> }`, `QueueFull`, `Failed(#[source] Box<dyn Error + Send + Sync>)`, plus `failed(err)`) and `Renderer`;
  - `RenderJob::of<T: Template>(&T) -> Result<RenderJob, ValueError>` and `decode_template<T: Template>(data: &[u8]) -> Result<T, ValueError>`;
  - the unit tests from the render matrix -- AC2.
- `crates/noctis-ports/src/lib.rs` -- crate docs (the op table, NFR-6 statement, namespacing note: adapters store keys verbatim and core's wrappers prefix), module tree, re-exports and `BoxFuture`.
- `crates/noctis-ports/tests/ports.rs` -- the ports matrix -- AC1.
- `crates/noctis-ports/tests/purity.rs`, modelled on the rules purity test:
  - forbidden tokens in non-comment lines of `src/`: `std::fs`, `std::net`, `std::io`, `std::process`, `std::thread`, `std::env`, `tokio`, `unsafe`, `PayloadKey`, `token`, `secret`, matched case-insensitively for the last two;
  - `[dependencies]` must be exactly `{bytes, postcard, serde, thiserror, twilight-model}`;
  - this test covers AC4 and AD-1's layer-1 property.
- `crates/noctis/Cargo.toml` and `crates/noctis/src/lib.rs`:
  - `pub mod ports` re-exports every public item of noctis-ports;
  - `pub mod bytes { pub use bytes::Bytes; }`;
  - module docs carry a small doctest that implements nothing but names `Arc<dyn CacheStore>` and round-trips `encode_value`/`decode_value`;
  - this covers AC5.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`. If the external `pub use` line breaks assembly, fix `api_snapshot.rs` with a unit test (that is the module's documented behaviour) -- AC5.
- `examples/project-template/.claude/skills/noctis/references/adapters.md`:
  - make §2 the real reference: drop "[planned exact signatures]";
  - add one signature block per port and the op-semantics table;
  - add `PortError`/`ValueError`, `encode_value`/`decode_value` and `Lease`;
  - note that `GuildAccessList::allows` is sync;
  - add `DiscordCache` getters and id lists, and a new section "Render contract (`Renderer`)" listing every render type, field, variant and method;
  - keep memory defaults, `ctx.kv()` and conformance marked [planned];
  - mention every new snapshot item.

  Then update the `SKILL.md` API hash -- AC5.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the planning decisions are already logged. Add implementation decisions only if any arise.

**Acceptance Criteria:**
- Given `noctis-ports`, when `cargo test --release -p noctis-ports` runs, then:
  - all 11 ports and `Renderer` are usable as `Arc<dyn _>`;
  - each store port has exactly its subset from the Boundaries table;
  - `DiscordCache` getters return owned `twilight_model` values;
  - every test in the three matrices passes.
- Given the render contract, when `noctis-ports` builds, then it has no takumi, `noctis-image` or other workspace dependency (`cargo tree -p noctis-ports -e normal` shows none), and the Template postcard round-trip test passes.
- Given a typed value, when it is stored with schema `s` and read with schema `s' != s`, then the read is a miss (`Ok(None)`).
- Given the noctis-ports source, when the purity test runs, then no port method can take a secret (no secret-typed or secret-named parameters) and `src/` does no I/O.
- Given the facade:
  - `noctis::ports::{RefStore, …, Renderer, PortError, BoxFuture}` and `noctis::bytes::Bytes` resolve;
  - `cargo run --release -p xtask -- api-snapshot` passes against the committed snapshot;
  - `cargo run --release -p xtask -- skill-check` passes (all items mentioned, hash current);
  - `deps-check` passes.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (all logged in the memlog):
- **Own `BoxFuture` alias.** It is identical to futures-util's, so the two are the same type for adapters. It avoids a dependency and an external re-export.
- **`GuildAccessList::allows` is sync.** AD-23 forbids port I/O in stage 3.
- **`LocaleSource::load` takes `&str`.** A `Locale` would need a ports → rules edge, which is not drawn.
- **`touch` takes `Duration`, not `Option`.** Touching to "no expiry" has no use case.
- **`ModuleStateStore` = get + put + version.** This is the skill's table. Core stores one blob per Module, so no enumeration is needed.
- **`DiscordCache` lists return ids.** A 100k-member guild must not be cloned per call.
- **`DiscordCache` getters return `twilight_model` types,** never `twilight-cache-inmemory`'s `Cached*`. Those types have private fields and are Serialize-only, so an out-of-process cache could not build them. Story 3.2 either converts the `Cached*` types or uses custom `CacheableModels` newtypes over full model types. The getter signature does not change either way.
- **Template data is plain postcard; port values carry the schema byte.** `template_version` already versions template data.
- **The OutputFormat extension for APNG is `png`.** Discord shows APNG only as `.png`.
- **The render types have no serde.** An out-of-process Renderer is not a v1 goal.

Sample of the shape:

```rust
pub trait RefStore: Send + Sync + 'static {
    fn put_if_absent<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>)
        -> BoxFuture<'a, Result<bool, PortError>>;
    fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    fn touch<'a>(&'a self, key: &'a str, ttl: Duration) -> BoxFuture<'a, Result<bool, PortError>>;
    // get, delete
}
```

## Verification

**Commands** (repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, and includes the noctis-ports unit, `ports` and `purity` tests and the facade doctests.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0 (postcard → cobs, serde; MIT/Apache).
- `cargo +1.91 check --release --workspace --all-targets` -- expected: exit 0 (MSRV; `Waker::noop` is stable since 1.85).
- `cargo tree --release -p noctis-ports -e normal --depth 1` -- expected: only bytes, postcard, serde, thiserror and twilight-model.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot` and `-- skill-check` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
