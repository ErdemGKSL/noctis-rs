# Changelog

All notable changes to Noctis are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Before 1.0, a breaking change bumps the minor version (`0.x` → `0.(x+1)`) and
everything else bumps the patch version.

## [Unreleased]

### Usage Skill (v1)

- `xtask skill-check` gains a completeness check: the required references
  (`concept-map.md`, `limits-cheatsheet.md`, `patterns.md`, `troubleshooting.md`) and
  their headings, a `SKILL.md` reference index that lists every reference file, no
  `planned`/`v0`/`Epic <n>`/`Story <n>.<m>` markers, a `## Rule id index` in
  `troubleshooting.md` that lists every `docs/discord-rules.md` rule id in catalog order
  with its meaning and fix, every id in `limits-cheatsheet.md`, a catalog link in every
  component entry's `**Limits:**`, and UD-7 over the Rust examples of the skill's and
  `docs/`'s Markdown. Success now ends with `, 135 rule ids indexed)`.
- New references: `references/patterns.md` (patterns with short code, anti-patterns,
  a common mistakes table with symptom, cause, fix and rule id) and
  `references/concept-map.md` (tescord / dbi and serenity / poise → Noctis).
- Every `[planned]` marker is resolved against the public API: the text now matches
  the real signatures, or says plainly what Noctis 0.1 does not provide (no `Ctx`
  trait, no `ctx.resolved()` on components, no `State<T>` parameters, no integer
  choices, no `FileFetcher` implementation, …). `SKILL.md` drops its v0 banner.
- UJ-5 agent trial (SM-3): in one round, 4 of 5 isolated agents built a leaderboard
  feature on the first attempt, learning Noctis only from the template and the skill. The
  most frequent gaps (`Kv` key listing, YAML colon spacing, `doc_markdown`) are now covered
  (`_bmad-output/implementation-artifacts/uj5-agent-trial-summary.md`).

### Rules catalog (v1 audit)

- `xtask/rules-pending.toml` is empty, and every entry it may hold must now carry
  `p2 = true` (a P2-feature requirement); `xtask rules-catalog` fails on an untagged
  `[[rule]]` or `[[check]]`.
- The CID-04 App-build tests in `noctis-core` cite their rule id (`cid_04_*`), so the
  last pending entry is gone: `rules-catalog: ok (135 rules: 130 covered, 0 pending,
  3 documentation-only, 2 aliases)`.
- `docs/discord-rules.md` has a `## v1 rules audit` section: the by-construction rules
  with their `// [<ID>]` evidence, every runtime-only rule with the reason its input is
  dynamic, and the documentation-only rows and aliases. The xtask unit test
  `real_catalog_audit_is_complete` keeps it equal to the classifier.
- PRD Q1 (Text Display counting unit), Q4 (media URL length) and Q5 (legacy to V2 on a
  modal-submit update) are still unanswered; the Unconfirmed values table names each
  with its conservative interpretation, and every `Limits` default is unchanged. Q5's
  conversion is pinned by `cv2_02_modal_submit_update_of_a_legacy_source_clears_it`.
- MSG-06 documents that no typed message carries `nonce` or stickers in v1 (the
  validators serve raw-REST callers); REG-06 reads "out of scope for v1".

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
  for a slash command (`noctis-core/tests/interaction_alloc.rs`), pinned
  on a one-worker runtime with `DiscordCacheConfig::none()` and no tracing
  subscriber or metrics recorder installed.
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

### Documentation

- New guides in `docs/guides/`: [Coming from tescord, dbi, serenity or
  poise](docs/guides/concept-map.md) (a concept map for people) and [Cluster
  deployment](docs/guides/cluster.md) (Shard Ranges, the shared `IdentifyQueue`, the
  HTTP proxy, shared Adapters, `NOCTIS_PAYLOAD_KEY`, one publisher). `README.md` has a
  `## Guides` section, and [`docs/guides/README.md`](docs/guides/README.md) indexes
  every guide.
- The `noctis` rustdoc builds with `RUSTDOCFLAGS="-D warnings"` with default and with
  all features (broken and ambiguous intra-doc links fixed), and docs.rs builds it with
  every feature (`[package.metadata.docs.rs] all-features = true`).
- New `xtask docs-check`: an offline link check (relative links, GitHub heading
  anchors, same-repo `blob/main` and `tree/main` URLs in the Markdown and in
  `crates/*/src`), every guide linked from `README.md` and the guides index, and no
  `ignore` doctest. It never fetches a URL. CI's new `docs` job runs it with both
  rustdoc builds and `cargo test --release -p noctis --all-features --doc`.

### Changed

- Component, command and autocomplete handler futures are boxed through a
  `map` combinator instead of an `async move` wrapper, which stored the
  handler's future twice inside its one allocation (about 1.4 KiB less heap
  per component interaction).

### Migration notes

- **First release.** 0.1.0 is the first published Noctis, so there is nothing to
  migrate from an earlier Noctis version.
- **Coming from tescord, dbi, serenity or poise.** The [concept
  map](docs/guides/concept-map.md) maps each concept to its Noctis item. The biggest
  differences:
  - every slash command, component, modal, event handler and lifecycle hook is one
    struct with a derive and a `#[handler]` method, registered in a `Module` (a
    tescord Pack);
  - component data is a typed Payload in the `custom_id` (`X { .. }.build()`), checked
    at compile time against Discord's limits; large data goes in a Ref with a TTL;
  - there are no collectors, await-loops or inline listeners: each component has its
    own handler, and its state travels in the Payload, a Ref or a port;
  - handlers answer with a Components V2 message through `ctx.respond(..)` or
    `ctx.update(..)`; deferring, flags and the response type are automatic, and there
    are no embeds;
  - command descriptions and every user-facing text come from typed locale files
    (`i18n!`, `Strings::of(ctx.user_locale())`), not from string literals;
  - commands are published with `--publish` / `--publish-guild` (or one
    `NOCTIS_PUBLISHER=true` process), never on READY;
  - storage goes through ports (`ctx.kv()`, `RefStore`, …) with bounded in-memory
    defaults; swap in your own Adapters for persistence.
- **Running several processes.** Follow the [Cluster deployment
  guide](docs/guides/cluster.md): the same shard total, a shared `IdentifyQueue`, one
  HTTP proxy, shared Adapters, the same `NOCTIS_PAYLOAD_KEY` and App id, and one
  publisher.
- **Pre-release adopters of the project template.** Values stored with `ctx.kv()`
  keep Discord ids as `u64` (`id.get()`, rebuilt with `Id::new_checked`), because
  twilight's `Id` does not decode from the Kv encoding. A value stored earlier with an
  `Id` field fails to read with `PortError::Value(Malformed)`: treat that error as
  absent and overwrite the value.
