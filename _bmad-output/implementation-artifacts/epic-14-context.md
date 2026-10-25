# Epic 14 Context: v1 launch readiness

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make Noctis releasable as v1 (0.1.0). Third-party adapters get reusable conformance suites. Benchmarks gate memory, latency and allocation. The Project Template and Usage Skill are complete and gated. The rules catalog has no pending rows. The launch documents exist, and the release pipeline passes a dry run. The last two stories cover repository setup and the crates.io publish, and both need the maintainer.

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

- **Conformance:** `noctis::testkit::conformance::<port>` covers the seven byte-store ports, `IdentifyQueue` and `DiscordCache` (not `LocaleSource` or `GuildAccessList`, which have no store semantics). The suites test TTL, overwrite, delete, missing key, concurrent atomicity (exactly one `take`/`put_if_absent` winner, no lost increments), `version()` monotonicity and `Namespaced<P>` namespacing. Every in-memory default passes its suite. Meta-tests show that broken implementations fail (one ignores TTL, one has a non-atomic `take`).
- **Template:** builds, passes clippy `-D warnings` and tests against the workspace. It covers `/dashboard`, `/setup start` (a hand-written step flow), a tabbed paginated View, `/profile` (a Template in a Media Gallery), `BoundedCacheStore` with its conformance test, and `en-US`/`tr` locales. The `template-build` job, the pixel snapshots and the adapter test become required gates. The "planned API" notes are removed. Handlers do not use embeds or plain `content` as primary output, select no response type by hand, and have no collectors or inline listeners. The lines of user code are reported against SM-1 (≤ 150).
- **Perf gates:** the NFR-3 latency bench measures p99 from event arrival to handler start (target ≤ 1 ms) at 10k events/s on 4 cores with the AD-25 mix. The dhat test checks that Route lookup and inline decode make zero allocations, and that each interaction makes at most one Payload port call and boxes one handler future. When a measurement misses its PRD target, `budgets.toml` uses the measured value + 10% and a note names the target. The latency, memory and allocation jobs switch to gating, and the changelog says which NFR assumptions were confirmed and which were re-set.
- **Rules audit:** `xtask/rules-pending.toml` is empty (only rows tagged `p2` may remain). Each Compile-time row has a trybuild `.stderr` containing its id. Each by-construction row has `// [<ID>]` evidence, and the expected list is in the story. Each App-build or Runtime row has a unit test citing its id. No rule with static input is checked only at runtime. Documentation-only rows are confirmed: CMD-07 discovery, REG-06, CMD-14/LOC-04 and MSG-02. Rows for Q1/Q4/Q5 are updated if those questions have answers.
- **Usage Skill:** mentions every public item and has an entry for every component (placement, Limits, builder), with links to `docs/discord-rules.md`. The `[planned]` and `v0` markers are removed. It adds a tescord concept map, a limits cheatsheet, troubleshooting indexed by rule id, patterns and anti-patterns, and common mistakes. The UJ-5 leaderboard trial needs at least 4 of 5 compiling runs, and its summary is stored in implementation-artifacts.
- **Docs:** `missing_docs` is denied on `noctis`. Examples are doctests. Sub-crate docs say "internal; use `noctis`". `docs/guides/` gets the tescord/dbi (plus serenity/poise) concept map and a Cluster deployment guide. `CHANGELOG.md` gets migration notes. The README links every guide, and a link check verifies them.
- **Release:** a final feature matrix (`image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `testkit`, plus `prefix` and `voice` once those epics have merged). Default features build without the image stack. `cargo-semver-checks` runs on `noctis`. release-plz publishes every crate except `xtask` at the shared version, in layer order, from the protected `release` environment that holds `CARGO_REGISTRY_TOKEN`. A token-less `cargo publish --dry-run` runs in CI. The release adds a 0.1.0 changelog entry and the runbook `docs/guides/release.md`.

## Technical Decisions

- Ports are dyn-compatible and return `BoxFuture<Result<T, PortError>>`. Only `Namespaced<P>` prefixes keys (`<app>:<port>:`), and adapters never do. Values are postcard with a leading schema byte.
- `xtask skill-check` fails if a public item is missing from the template's `.claude/skills/noctis/` or if the API snapshot changed without a new API hash. Every public API change updates the template, its skill and `xtask/public-api.txt` in the same commit.
- Benches live in `noctis-core/benches` (the render bench is in `noctis-image/benches`). Story 14.3 is done: every `budgets.toml` table has `gate = true`, and NFR-1, NFR-3 and NFR-9 were confirmed (event p99 about 164 µs, interaction p99 about 285 µs, 0 allocations for Route lookup and decode). The NFR-3 gate is the criterion-free `dispatch_latency` bench (exit 0 ok, 1 over budget, 2 invalid measurement); allocation budgets are exact pins in the dhat test `noctis-core/tests/interaction_alloc.rs`, never `budgets.toml` tables. Handler futures are boxed via a `map` combinator, never an `async move { fut.await }` wrapper.
- CI is paused in `.github/workflows-disabled/`. Before the launch stories, move the workflows back to `.github/workflows/` and fix every failure. A story is done when all local gates pass.
- Toolchain 1.97.0, MSRV 1.91, `resolver = "3"`. The `deny.toml` license allow-list is fixed, and a new license needs the maintainer's approval.
- Template Kv values store twilight ids as `u64` (postcard rejects `Id`'s `deserialize_any`); a framework-level guard is a public API change that belongs to Story 14.5, not the rules audit. `noctis::id` and prelude `Id` exist. There is no shared `Ctx` trait: helpers take the accessors they need.
- The template gate (fmt, build, clippy, `CI=true` test; default and `--all-features`; `--locked`) runs in `ci.yml`'s `template-build` and in `tools/story-loop/gates.sh`. SM-1 is reported in the template README, not enforced.
- Rules audit (14.4): every `rules-pending.toml` entry must carry `p2 = true` or `rules-catalog` fails. The catalog gets a v1 audit section (by-construction evidence, runtime-only rows with why their input is dynamic, documentation-only/alias rows), kept equal to the classifier by an xtask unit test. LOC-04 stays Compile-time by construction (Gate 2's 25 plus LOC-04); INT-05 stays diagnosed. PRD Q1/Q4/Q5 remain unanswered, so `Limits` defaults stay; Q5 follows the maintainer rule that a modal-submit update converts a legacy source to V2 like a component update.
- Epic 14's template, skill and docs stories cover the `voice` feature once Epic 17 has merged.

## Cross-Story Dependencies

- 14.1 provides the adapter conformance test that 14.2 makes a required gate.
- 14.7's runbook lists the maintainer steps for 14.8 and 14.9. Unattended runs skip 14.8 and 14.9, report them as waiting and continue with Epic 15.
- 14.9 switches the template to `noctis = "0.1"` after the publish and marks Milestones 1–4 done in the README roadmap.
