# Epic 14 Context: v1 launch readiness

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Bring Noctis to a releasable v1: third-party adapters can prove their semantics with reusable conformance suites, benchmarks gate memory, latency and allocation, the Project Template and Usage Skill are complete and enforced, the Discord rules catalog has no pending rows, launch documentation exists, and the release pipeline is dry-run green so that publishing `noctis` 0.1.0 needs only maintainer repository setup and one merge.

## Stories

- Story 14.1: Adapter conformance suites
- Story 14.2: Project Template builds, runs and gates CI
- Story 14.3: Dispatch latency, allocation and memory gates
- Story 14.4: Rules catalog completeness audit
- Story 14.5: Usage Skill completeness and agent trial
- Story 14.6: Launch documentation
- Story 14.7: Release pipeline ready for 0.1.0
- Story 14.8: [Maintainer-gated] Repository settings and release secrets
- Story 14.9: [Maintainer-gated] Publish `noctis` 0.1.0

## Requirements & Constraints

- **Conformance:** `noctis::testkit::conformance::<port>` per store port, `IdentifyQueue` and `DiscordCache`; checks TTL, overwrite, delete, missing key, concurrent atomicity (single `take`/`put_if_absent` winner, no lost increments), `version()` monotonicity, App namespacing via `Namespaced<P>`. Every in-memory default passes; deliberately broken impls must fail (meta-tests).
- **Template (SM-1):** builds, clippy-clean and tested against the workspace; covers `/dashboard`, `/setup start` step flow, tabbed paginated View, `/profile` Template in a Media Gallery, `BoundedCacheStore` adapter, `en-US`/`tr` locales. User code for sharding, auto-published Commands, 3-step setup and rendered card reported against ≤ 150 lines. No embed/plain-content primary output, no manual response-type selection, no per-message collectors or listeners (UI Doctrine, NFR-2).
- **Performance (NFR-1/3/9, SM-4):** memory ≤ 1 KiB per idle guild (excluding Discord cache) as initial goal; dispatch p99 ≤ 1 ms at 10k events/s on 4 cores; Route lookup and fixed-width inline decode allocate nothing; at most one port call for the Payload and one boxed handler future per interaction. Missed targets: gate = measured + 10%, with a note naming the PRD target; changelog records confirmed vs re-set assumptions.
- **Rules coverage (NFR-10):** static input always checked at compile time; every enforced rule has a documented Enforcement Point and test evidence citing its id; `rules-pending.toml` empty except `p2`-tagged rows.
- **Usage Skill (SM-3):** mentions every public item and every component (placement, Limits, builder), links `docs/discord-rules.md`, drops `[planned]`/`v0` markers; includes tescord → Noctis concept map, limits cheatsheet, troubleshooting by rule id, patterns/anti-patterns, common mistakes. UJ-5 leaderboard task must compile without reading Noctis source in ≥ 4 of 5 agent trials; summary stored in implementation-artifacts.
- **Docs:** `missing_docs` denied on `noctis`; runnable examples are doctests; sub-crates say "internal; use `noctis`"; `docs/guides/` has migration concept map (plus serenity/poise section) and Cluster deployment guide; README links every guide (link-checked); `CHANGELOG.md` with migration notes.
- **Quality gates (NFR-8):** every crate and the template pass fmt, clippy `-D warnings`, tests and doctests; renderer upgrades merge only after template pixel snapshots pass.

## Technical Decisions

- **Ports:** dyn-compatible, `Send + Sync + 'static`, `BoxFuture<'_, Result<T, PortError>>`; only core's `Namespaced<P>` prefixes keys; adapters never prefix.
- **Benchmarks:** live in `noctis-core/benches` with `benches/budgets.toml` (one table per bench: `nfr`, `guilds`, `gate`, per-metric `max` and required `note`; unknown keys rejected). `memory_per_guild` exists (Story 12.6) with `gate = false`; Story 14.3 turns gating on. dhat counting allocator for allocation test; latency bench uses the 60/25/10/5 event mix with no-op handlers. Reference card render bench in `noctis-image/benches` reads the same file.
- **Release (AD-28):** semver, pre-1.0 breaking changes bump minor with migration notes; `noctis::image::unstable` outside semver; upstream-wrapping public enums `#[non_exhaustive]`. release-plz publishes all crates except `xtask` at the shared version in layer order from a job bound to the protected `release` environment holding `CARGO_REGISTRY_TOKEN`; `cargo publish --dry-run` in layer order must pass tokenless. Toolchain stable 1.97.0, MSRV 1.91, `resolver = "3"`, `--release` everywhere, `cargo-semver-checks` and `cargo public-api` in CI only.
- **CI:** workflows currently paused in `.github/workflows-disabled/`; before Epic 14's launch stories they move back to `.github/workflows/` and every failure is fixed. Gates: fmt, clippy, tests, `cargo hack --each-feature` plus all-features build, `missing_docs`, MSRV, trybuild, semver-checks, cargo-deny, xtask deps-check/rules-catalog/api-snapshot/skill-check, tw-crosscheck, allocation test, insta and template pixel snapshots, template build.
- **Feature matrix:** `image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `testkit` (`prefix` once Epic 15 lands); default features build without the image stack.
- Every public API change updates the template, its Usage Skill and `AGENTS.md`, re-blesses `xtask/public-api.txt` and the skill's API hash.

## Cross-Story Dependencies

- 14.1's template `tests/adapters.rs` becomes a required gate in 14.2.
- 14.3 builds on the Story 12.6 memory bench.
- 14.7's runbook `docs/guides/release.md` drives 14.8 and 14.9; 14.6's changelog feeds 14.7's 0.1.0 entry.
- 14.8 and 14.9 are maintainer-gated: unattended runs skip them, report them waiting, and continue with Epic 15. 14.9's follow-up switches the template to `noctis = "0.1"` and marks README Milestones 1-4 done.
