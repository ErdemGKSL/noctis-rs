---
type: implementation-readiness
project: noctis-rs
date: 2026-10-19
gate: bmad-sprint-planning / references/readiness-gate.md
verdict: FAIL
gates:
  - gate: 1
    verdict: CONCERNS
  - gate: 2
    verdict: FAIL
---

# Implementation Readiness: Noctis (noctis-rs)

This file holds two readiness gates. Gate 1 is kept unchanged, together with its
Resolution section. Gate 2 re-checks those resolutions and looks for new issues; it
holds the current verdict.

## Gate 1 (2026-10-19)

### Verdict: CONCERNS

Epic 1 can start now. The plan traces cleanly in both directions, and the
`custom_id` design is consistent everywhere I checked. But the artifacts disagree on
four points, and a developer would have to invent the answer for each one partway
through the plan:

- what `CacheStore` means, and whether a separate `DiscordCache` port exists;
- where Command descriptions come from (a literal in the attribute, or the locale files);
- what a `RenderJob` contains, and where the `Template` trait lives;
- how the repository rule "release builds only" fits with trybuild, which always builds in the dev profile.

There are also several API-shape conflicts between the spine and stories on one side
and the Usage Skill and Project Template on the other (Responder, `#[route]` attributes).

Recommendation:
- Start Epic 1. Settle findings H-4, M-5 and L-1 inside Stories 1.1 and 1.7.
- Run `bmad-correct-course` for H-1 to H-3, M-1 and M-2 before Epic 3 starts (Story 3.1 defines `CacheStore` and `RenderJob`).
- Everything else must be settled before Epic 6.

### Scope and method

Artifacts read:
- `briefs/brief-noctis-rs-2026-10-19/` (skimmed; superseded by the PRD);
- `prds/prd-noctis-rs-2026-10-19/prd.md` and `addendum.md`;
- `architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md`;
- `epics.md` (15 epics, 96 stories);
- `research/` (takumi research, and the architecture's `review-versions.md` for the pins);
- `docs/discord-rules.md`;
- `examples/project-template/`: code, `Cargo.toml`, `AGENTS.md`, the Noctis Usage Skill and its references;
- root `AGENTS.md`.

There is no UX document. The UI Doctrine UD-1 to UD-8 is mapped as UX-DR1 to UX-DR9.
That is acceptable, because every UI story cites a UX-DR.

How each part was checked:
- **Read in depth:** Epics 1, 2, 3, 4, 5, 6, 7, 8 and 10, plus Stories 11.x, 12.5, 13.1 to 13.4 and Epic 14.
- **Traceability:** checked with scripts. FR, NFR and AD ids were matched against story `Implements:` lines. Catalog rule ids were matched against every citation in `epics.md`, with ranges such as `OPT-01..OPT-14` expanded.
- **trybuild's behavior:** checked in the source of trybuild 1.0.122, downloaded from crates.io.

### What is solid

- **Traceability, forward:**
  - FR-1 to FR-79, NFR-1 to NFR-10 and AD-1 to AD-29 each appear in at least one story `Implements:` line.
  - All 123 rule ids in `docs/discord-rules.md` are cited by a story.
  - No story cites a rule id that is missing from the catalog.
  - The FR Coverage Map in `epics.md` matches the stories.
- **Traceability, backward:** every story traces to an FR, NFR, AD or rule id. The exceptions are template and skill API names that no story owns (L-6).
- **The `custom_id` format agrees in four places:** AD-5 and AD-6, `docs/discord-rules.md`, Stories 2.2 and 2.5, and the skill's `routes-payloads-refs.md`.
  - They share the same layout, constants, `MAX_LEN` formula and field widths.
  - I recomputed every worked example and all are correct: `BuyButton` 24, `ToggleModule` 25, `PickLogChannel` 36, `SetupState` 49, and the by-ref worst case 46.
- **Names:**
  - Crate names are consistent across the spine, Story 1.1, the deps graph and the template's path dependency.
  - Facade feature names are consistent across the spine Conventions, Story 1.1 and the template `Cargo.toml`.
- **Template excluded from the workspace:** the plan is consistent on this, and the template agrees.
  - The template has its own empty `[workspace]`.
  - Both `AGENTS.md` files and Story 1.1 require `exclude = ["examples/project-template"]`.
  - The template's dependency is `path = "../../crates/noctis"` with the features Story 1.1 creates.
  - Story 1.1 runs it in tracking mode until Story 14.2.
- **Toolchain:**
  - Edition 2024 and MSRV 1.91 are consistent everywhere.
  - The Stack versions were checked against crates.io in `reviews/review-versions.md`.
- **Release-only rule:** stated in `AGENTS.md`, in the epics Story conventions and in every cargo command in Story 1.1.
- **Story format:** every story has BDD acceptance criteria with typed-error and boundary cases. Each story that changes the public API has an explicit criterion to update the template and the skill.

### Findings (ordered by severity)

Severity levels:
- **High:** a developer must invent a decision that crosses crates or artifacts. Resolve it before the affected epic starts.
- **Medium:** a conflict or gap that a story will hit. Resolve it in the story or with a small correction to the plan.
- **Low:** hygiene.

#### H-1 `CacheStore` means three different things, and `DiscordCache` is half-defined

**Where:**
- **PRD:**
  - `prd.md` FR-38 (§4.6, around line 920) defines two ports: `CacheStore`, a shared key-value store with TTL for bots and Modules, and `DiscordCache`, the backend of the Discord resource cache.
  - FR-40 and FR-71 depend on that split.
- **Spine:** `ARCHITECTURE-SPINE.md` AD-1 lists `CacheStore (Discord cache backend)` and has no `DiscordCache` and no bot-facing key-value port.
- **Epics:**
  - The FR inventory (line 183) lists both ports.
  - "Additional Requirements" (line 273) and Story 3.1 follow the spine's list.
  - Story 12.5 says "an out-of-process implementation of the cache port" without naming the port.
- **Usage Skill:** `references/adapters.md` defines `CacheStore` as both a shared key-value store reached through `ctx.cache()` and "the backing store of the Discord resource cache". It also defines `ctx.discord()` and `DiscordCacheConfig`.
- **Template:**
  - `src/lib.rs` calls `.cache_store(..)`.
  - `src/modules/profile.rs` and `setup.rs` use `ctx.cache().put / incr / counter`.
  - `src/adapters/bounded_cache.rs` implements it.
  - `tests/adapters.rs` runs `conformance::cache_store`.

**Problem:**
- Three artifacts define the port differently.
- No story delivers the typed `ctx.cache()` handle. Its methods `get`, `put`, `put_if_absent`, `incr` and `counter` appear nowhere in the epics, yet the template's main flows use it.
- Nothing records how the Discord resource cache works:
  - The Stack pins `twilight-cache-inmemory`, which is typed and in-process and hands out reference guards.
  - FR-40 requires both "owned values, no guard across `.await`" and "an out-of-process implementation serves the whole Cluster".
  - Putting an async byte store under a typed Discord cache is a major design decision that nothing makes.

**Fix:**
1. Run `bmad-architecture` (update mode) and amend AD-1 and AD-14. Decide:
   - whether `CacheStore` is the application key-value store;
   - whether `DiscordCache` is its own port, or whether the out-of-process Discord cache is deferred;
   - which ops each port exposes.
2. Run `bmad-correct-course` to update Stories 3.1 and 12.5, and to add a story (or acceptance criteria in Story 5.1 or 7.4) for the typed `ctx.cache()` handle.
3. Update the skill's `adapters.md` to match.

#### H-2 Where a Command's description comes from is contested

**Where:**
- `docs/discord-rules.md` CMD-04 says descriptions come from `description = "…"` or `///` doc comments, measured in the macro, and a missing one is a compile error.
- Story 6.1 uses `#[command("settings logs channel", description = "…")]`, with trybuild fixtures for a missing or 101-character description.
- AD-16, Story 11.4, the skill's `references/commands.md` (lines 34 and 119) and the template's `locales/en.yaml` take descriptions only from `cmd.<path>.desc` in the default locale. The skill adds: "Doc comments are rustdoc only". The template's `#[command]` attributes carry no `description`.
- Context-menu names: the skill and template use `key = "show_profile"` and `ctxcmd.<key>.name`, but AD-16's fixed key list has no context-menu key.

**Problem:**
- Epic 6 comes before Epic 11.
- If Story 6.1 follows the spine and skill, `#[command]` must read the crate's locale files at compile time in Epic 6. Locale compilation (`i18n!`) does not exist until Story 11.1.
- If it follows the catalog and story, the template and skill are wrong, and Epic 11 must later define a precedence rule between the attribute literal and the locale key.
- Either way, the developer of Story 6.1 picks a public API.

**Fix:** run `bmad-architecture` to amend AD-16 with:
- the single source of descriptions;
- the precedence when both sources exist;
- the context-menu key layout;
- which epic first reads locale files at compile time.

Then use `bmad-correct-course` to update Story 6.1, Story 11.4 and the CMD-04 catalog row (the catalog defers to the spine).

#### H-3 The `Renderer` seam has no `RenderJob` shape, and `ctx.render` crosses a forbidden edge

**Where:**
- AD-22 and AD-15 say `Renderer` is a driven port in `noctis-ports` that takes a `RenderJob` and returns `RenderOutput`.
- Story 3.1 must declare `RenderJob` "without depending on takumi".
- Story 10.3 defines `ctx.render(template, data).await` in core.
- Story 10.2 puts `#[derive(Template)]` in the image stack.
- The Invariants graph has no `core → image` edge.

**Problem:**
- Nothing records what a `RenderJob` carries. Candidates include a template id with postcard data, a serialized node tree, or a boxed closure.
- Nothing records where the `Template` trait lives, so that core can be generic over it without depending on `noctis-image`.
- Story 3.1 (Epic 3) must freeze the type long before Epic 10 knows what it needs. That is a design-level forward dependency.

**Fix:** use `bmad-architecture` to add the `RenderJob`, `Template` and `ctx.render` contract to AD-22, including the crate that owns each trait. Then use `bmad-correct-course` to make Story 3.1 define only what the contract fixes, and Stories 10.2 and 10.3 build on it.

#### H-4 The "release builds only" rule conflicts with trybuild

**Where:**
- Root `AGENTS.md` says "never create a debug build".
- The `epics.md` Story conventions say "no story creates a debug build".
- Story 1.7 adds the trybuild harness, and every compile-time story adds fixtures (2.5, 4.1, 4.4, 4.6, 6.1 to 6.3, 7.8, 10.2, 11.1).

**Verified in trybuild 1.0.122 (`src/cargo.rs`, lines 97, 130 and 153):**
- trybuild runs `cargo build` (when pass tests exist) or `cargo check`.
- It never passes `--release`.
- It sets `CARGO_TARGET_DIR=target/tests/trybuild`.
- It does not copy the workspace's profiles.

The fixture project and the whole facade dependency tree are therefore built in the dev profile into a second target directory. If UI tests enable `image`, that includes takumi. `cargo public-api` and `cargo semver-checks` also build rustdoc JSON outside `--release`.

**Problem:** Story 1.7 cannot meet both its own criteria and the repository rule. The developer would invent either an exception or a workaround.

**Fix:** record an explicit exception for trybuild, `cargo public-api` and `cargo semver-checks`, with mitigations:
- run UI tests on default features where possible;
- set a lean dev profile (`[profile.dev] debug = 0`, or `CARGO_PROFILE_DEV_DEBUG=0` in CI);
- delete `target/tests/` after UI runs in cloud sessions.

Put the exception in root `AGENTS.md` (via `bmad-project-context`) and in the epics Story conventions and Story 1.7 (via `bmad-correct-course`).

#### M-1 The Responder API differs between spine and stories on one side and the skill on the other

**Where:**
- **Spine, catalog and stories:** AD-11, catalog rows INT-03, INT-04 and INT-05, and Stories 6.5, 7.2 and 7.8 use `Responder<Pending>` → `Responder<Acked>`. `modal` exists "on contexts of handlers declared `opens_modal`", and "`AutocompleteCtx` offers only `respond`".
- **Usage Skill:** `references/responder.md` §4 differs:
  - it uses `Responder<_, Acked>`, with a type parameter for the handler kind;
  - the Responder is taken as a handler parameter through the aliases `CommandResponder`, `ComponentResponder` and `ModalResponder`;
  - `acked.done() -> Responded` and `followup(msg, Visibility)` return a `FollowupHandle`;
  - autocomplete "returns `Suggestions`".

**Problem:**
- The INT-05 trybuild fixture in Story 7.8 says a handler not declared `opens_modal` that calls `modal` fails to compile. That needs `opens_modal` encoded at the type level (an extra type parameter or a distinct type), and nothing records how.
- Where `opens_modal` is declared differs:
  - on the route attribute (`#[route("profile.bio", opens_modal)]`) in the skill, the template and the catalog's INT-05 row;
  - on "handlers" in AD-11;
  - on "a Command or Route" in Story 7.8.

**Fix:** use `bmad-architecture` to pin the Responder's generic signature, how the context is injected, and where `opens_modal` is declared, in AD-11. Then use `bmad-correct-course` to align Stories 6.5, 6.7 and 7.8 and the skill's `responder.md`.

#### M-2 The `#[route]` attribute grammar is incomplete, and parts of it have no story

**Where:**
- AD-6 ("One attribute syntax") and Story 2.5 list only `by_ref`, `overflow = spill`, `schema = n`, `view` and `signed`.
- The skill (`routes-payloads-refs.md`, `#[route(...)]` arguments) and the template also use `select`, `ttl = "…"`, `one_shot`, `opens_modal`, `owner = field`, `permissions = …` and `guard = fn`.
- Story 7.5 uses `one_shot`, a per-Route TTL and `overflow = error`.
- Story 7.7 uses `owner` and `permissions`.
- Story 13.4 puts `rate_limit(..)` on the Route. The skill puts it on `#[handler(...)]`, together with `response = …`, `on_expired` and `on_stale`.

**Problem:**
- `select` sets the `N` nonce section, which AD-5, Story 2.2's shape check and Story 8.2's stale-select behavior all depend on. No story adds the `select` attribute.
- `overflow = error` (Story 7.5, from PRD FR-17) contradicts AD-6: an inline Route that can overflow does not compile, so no runtime "error" policy can exist. The skill does not list it.
- Nothing decides which arguments belong to the codec derive (Epic 2) and which belong to runtime registration (Epic 7). The derive in Story 2.5 has to accept, or reject, attributes that later epics define.

**Fix:**
1. Use `bmad-architecture` to publish the full `#[route]` and `#[handler]` grammar in AD-6 and AD-19, split into codec attributes and runtime attributes. State that FR-17's "error" policy is satisfied at compile time.
2. Use `bmad-correct-course` to add `select` to Story 2.5 (or to Story 8.2, as an explicit extension), drop `overflow = error` from Story 7.5, and say which story adds each runtime attribute.

#### M-3 Forward dependencies the validation summary does not list

**Where:**
- **Story 7.6 (Epic 7) needs nested Modules from Epic 13.** Its criteria need "before-Middleware registered on a parent Module and a child Module", and its App-build check looks at "every ancestor path" (AD-23). Nested Modules (`Module::new(..).module(..)`) arrive only in Story 13.1, which repeats "parent-before-child ordering applies to Middleware". This crosses epics.
- **Story 4.1 needs the handler macro from Epic 7.** Building a component positively needs `R: Handled`, "which only the handler macro implements" (Story 7.1). Epic 4 has no recorded way to satisfy it in its own tests, such as a hand-written or test-only impl.
- **Story 5.1 uses Modules before Story 5.4 defines them.** 5.1's criteria set "Modules" on the builder, but `Module::new` is defined in 5.4. This one stays inside the epic.

The validation summary in `epics.md` says the only cross-epic mentions are 1.7 → 2.2, 1.8 → 4.1, 2.6 → 7.1, 5.6 → 13.5 and 12.6 → 14.3.

**Fix:** use `bmad-correct-course` or `bmad-create-epics-and-stories`:
- move basic Module nesting into Story 5.4, or move the parent/child criteria of 7.6 into 13.1;
- state how Epic 4 tests satisfy `Handled`, for example a `#[doc(hidden)]` test impl or an unsealed trait;
- reorder or reword Story 5.1.

#### M-4 No test harness or HTTP seam is designed for the Discord mock

**Where:**
- Epic 5's intro says tests use "an HTTP mock server plus an in-process gateway stub".
- Stories 5.2, 6.9, 7.8 and 8.6 rely on mocked HTTP and the gateway stub.
- Story 8.6's testkit must assert the response kind (reply, update, deferred, modal) and ephemerality.
- AD-24 says no crate except core constructs the Discord HTTP client.

**Problem:**
- Nothing names the mechanism. It could be pointing twilight-http at a mock server through `proxy(url, true)`, a transport trait in core, or something else.
- No mock or websocket crate is in the Stack.
- No crate owns the harness. Epics 5, 6 and 7 build it ad hoc, and Epic 8 builds it again as the testkit.

**Fix:** use `bmad-architecture` to add a test-harness decision to AD-10 or AD-24: the seam, the crate that owns it (likely `noctis-testkit` from Epic 5) and Stack pins. Then use `bmad-correct-course` to make the first Epic 5 story create it.

#### M-5 No stable toolchain pin, and no way to keep UI tests on 1.91

**Where:**
- The spine's Stack pins MSRV 1.91 and `nightly-2026-10-15`, but no stable toolchain for clippy and tests.
- Story 1.1 has no `rust-toolchain.toml` decision.
- Story 1.7 says UI tests "run on the 1.91 toolchain only in CI".
- The epics Story conventions say "Done means … `cargo test --release`".

**Problem:**
- trybuild `.stderr` output changes between compiler versions. The takumi research ran on rustc 1.97.
- With no gate (an env var, a `cfg`, an ignored test, or a toolchain file), a developer's local `cargo test --release` on current stable fails the UI tests.
- Clippy with `-D warnings` will drift with whatever stable CI uses.

**Fix:** use `bmad-correct-course` on Stories 1.1 and 1.7. Either pin `rust-toolchain.toml` to 1.91 for the workspace, or gate the trybuild test, for example `#[ignore]` unless `NOCTIS_UI_TESTS=1`, and pin a stable version for clippy in CI.

#### M-6 Some "compile-time" checks only fire under `cargo build`

**Where:**
- Story 4.4: "checked by `cargo check`", using `ActionRow::buttons([b; N])` and `Section::new([t; N], …)` with const asserts on `N`.
- Catalog rows CV2-07 ("const assert on N") and GW-06 (`LargeThreshold::new::<N>()`).

**Problem:**
- A `const { assert!(..) }` that depends on a generic parameter is evaluated at monomorphization. `cargo check` and rust-analyzer do not report it. AD-6 itself notes this for Routes.
- The trybuild fixtures still fail, because trybuild uses `cargo build` when the harness also has pass tests. The tests are green while authors running `cargo check` see nothing.
- Placement rules enforced by marker traits (CV2-05) produce rustc type errors, not text carrying a rule id. Putting `[CV2-05]` in the `.stderr`, as the Conventions and Story 1.7 require, needs `#[diagnostic::on_unimplemented]`. No artifact records that.

**Fix:** use `bmad-architecture` to add both mechanisms to AD-4 (post-monomorphization limits, `diagnostic::on_unimplemented`). Then use `bmad-correct-course` to reword the Story 4.4 user story and catalog rows CV2-06, CV2-07, CV2-10 and GW-06 ("fails at build" where that is the truth), or switch them to non-generic const constructors.

#### M-7 Missing alt text is a warning in Story 4.3 and an error in Story 9.1

**Where:**
- Story 4.3 says a Strict-mode Thumbnail or gallery item without alt text is a warning in the cost report (FR-24).
- Story 9.1 says an image in a Thumbnail or Media Gallery without alt text is a validation error in Strict mode.
- PRD FR-30 limits the error to **rendered** images.

**Problem:** the validator cannot tell a rendered image from any other, and no marker for that is recorded (for example `Attachment::with_digest` provenance).

**Fix:** use `bmad-correct-course` to scope Story 9.1 to rendered attachments, record the marker in AD-22, and keep Story 4.3's warning for everything else.

#### M-8 Locale and theme file handling is unrecorded

**Where:** AD-16, the Stack, Story 11.1, the template's `src/strings.rs` and `locales/*.yaml`, Story 10.2 (`noctis-theme.css`), and the `noctis-macros` row in the layer table ("reads the invoking crate's locale and theme files").

**Problem:**
- **YAML parser:** the format is YAML in the template, skill and Story 11.1, but no parser is in the Stack. `serde_yaml` is archived and carries an "unmaintained" RustSec advisory, so the cargo-deny gate from Story 1.1 would flag it.
- **CSS parser:** none is chosen for the theme file.
- **Rebuild tracking:** proc macros that read files need it (for example emitting `include_str!`), or edits to locale and theme files leave builds stale. This is not recorded.
- **`i18n!` syntax:** AD-16 shows `i18n!("locales")`, while Story 11.1 and the template use `i18n!(default = "en", dir = "locales")`.

**Fix:** use `bmad-architecture` to add the file formats, parser crates and file-tracking rule to AD-16, AD-15 and the Stack, and settle the `i18n!` syntax.

#### M-9 Nothing records who owns command-line arguments

**Where:**
- Story 6.9: `--publish`, `--publish-guild`, `--publish-clear` and `--dry-run`.
- Story 13.2: `--features`.
- Story 12.3: `--no-resume`.
- The template's `main.rs`, where `App::run` handles all of them.

**Problem:** `App::run` parses the process's own arguments, and nothing records:
- the parser;
- how a bot adds its own flags;
- how an author opts out, or passes arguments explicitly (needed for tests and for running several Apps).

**Fix:** use `bmad-architecture` to add a convention to the Config row (for example `App::run_with_args(impl IntoIterator)`, a `noctis::cli` parser struct, or no clap dependency). Then use `bmad-correct-course` to update Story 6.9.

#### L-1 The virtual workspace needs `resolver = "3"`

**Where:** Story 1.1 and the spine's Structural Seed. The root is a virtual manifest, with members under `crates/`.

**Problem:** a virtual workspace does not take its resolver from member editions. Cargo defaults to resolver 1 and warns. Resolver 1 also unifies dev-dependency features into normal builds, so the `testkit` feature can leak into the per-feature build matrix and the "default features build without image" check.

**Fix:** add `resolver = "3"` to the criteria of Story 1.1 (`bmad-correct-course`, or have the developer record it in the story).

#### L-2 Small choices left to Story 1.1

**Where:** Story 1.1.

**Problem:** the developer must choose:
- the contents of `[workspace.lints]`;
- the `deny.toml` license allow-list;
- that release-plz skips `xtask` (`publish = false`);
- a placeholder file so `docs/guides/` exists in git.

One license point matters later, for Epic 10: the `default-font` and Twemoji assets used by `image-emoji` (Twemoji graphics are CC-BY-4.0) need a recorded license policy.

**Fix:** these are acceptable developer choices. Record them in the story's dev notes, and record the asset licenses when Epic 10 starts.

#### L-3 The reference-card benchmark needs a forbidden edge

**Where:** AD-25 and Story 10.11. The reference-card render bench lives in `noctis-core/benches`.

**Problem:**
- The bench needs `noctis-image`.
- Story 1.2's `deps-check` allows dev-dependencies only on the `noctis` facade.

**Fix:** route the bench through a dev-dependency on `noctis` with the `image` feature, or move the bench to `noctis-image`. Record the choice in AD-25.

#### L-4 Workspace membership of focused examples is undefined

**Where:** the Seed's `examples/<feature>/` and Story 8.7's `examples/views/`.

**Problem:** these are neither in Story 1.1's members nor excluded, and nothing says whether they are workspace members or standalone like the template.

**Fix:** decide in Story 8.7 (`bmad-correct-course`).

#### L-5 Epics 2 to 4 are technical layers

**Where:** Epics 2, 3 and 4.

**Problem:** these epics deliver no runnable bot. Epic 5 is the first that does.

**Fix:** none needed. This is acceptable for a framework, and the epic order follows the crate layers by design.

#### L-6 Template and skill API names that no story owns

**Where:** the template and the Usage Skill use:
- `IntoResponse` and `Responded`, `ViewCx`, `Nav::` with `.at(..)`, `.with_notice(..)`;
- `ModuleScope`, `ref_ttl`, `ctx.discord()`;
- `noctis::image::theme!()`, `.locales(..)`;
- `#[autocomplete]`, `Suggestions`, `Visibility`.

**Problem:**
- Stories only say "reconcile the template with the real API", which is fine for naming.
- `#[handler(response = ephemeral | public)]` is different: a component that opens a **new** message is a behavior. It has no FR, no AD-11 rule and no story.

**Fix:** use `bmad-correct-course` to add acceptance criteria where these names are behaviors, or remove them from the skill.

#### L-7 Skill wording about the API hash conflicts with Story 1.8

**Where:** `SKILL.md` line 21 and Story 1.8.

**Problem:** the skill says its API hash is set "once `noctis` is published", but Story 1.8 sets it in Epic 1.

**Fix:** sync the text in Story 1.8.

#### L-8 What the `i18n` facade feature gates is unclear

**Where:** the Invariants graph and the facade feature list.

**Problem:** the graph makes `noctis-core → noctis-i18n` an unconditional edge, so it is unclear what the `i18n` facade feature switches on (presumably `i18n!`).

**Fix:** say it in AD-16 or the Conventions.

### Epic 1 Story 1: can it be built now?

Yes, with the decisions listed under "Needs a decision" below. Check against the actual repository (no `Cargo.toml` and no `.github/` exist yet):

| Item | Status |
| --- | --- |
| Workspace layout (12 crates plus `xtask`, matching the Seed) | Specified exactly in Story 1.1 and Additional Requirements |
| `exclude = ["examples/project-template"]` | Consistent: Story 1.1, both `AGENTS.md` files, and the template's own `[workspace]` |
| Template's path dependency and features exist on day one | Yes: `image`, `default-font`, `i18n` and `testkit` are created empty in Story 1.1 |
| Edition, MSRV and Stack pins | Consistent and verified (review-versions) |
| Release-only cargo commands in CI | Specified; conflicts with trybuild (H-4) from Story 1.7 on |
| Toolchain for stable clippy and tests | Missing (M-5) |
| `resolver` | Missing (L-1) |
| `deny.toml`, `release-plz.toml`, lints | Contents left to the developer (L-2) |
| `template-build` tracking job | Specified (non-blocking until Story 14.2) |
| Updating `AGENTS.md` | Specified |

### Story independence and size

- Most stories are independently completable inside their epic. Exceptions are listed in M-3.
- Stories that are oversized or high-risk:
  - **Story 1.1:** workspace, CI matrix, deny and release stubs. Large but mechanical.
  - **Story 10.1:** `xtask tw-grammar` "generates the grammar from takumi's source", which is research-heavy. Consider a spike story first.
  - **Story 14.5:** five agent trials. This is validation, not development.
- Stories 5.2 and 8.6 grow by the size of the harness from M-4 until it is settled.

### Fix routing summary

| Finding | Skill |
| --- | --- |
| H-1, H-2, H-3, M-1, M-2, M-4, M-6, M-8, M-9 | `bmad-architecture` (update the spine), then `bmad-correct-course` for the affected stories, the skill and the catalog |
| H-4 | `bmad-project-context` (root `AGENTS.md`) and `bmad-correct-course` (Story conventions, Story 1.7) |
| M-3, M-5, M-7, L-1, L-4, L-6, L-7 | `bmad-correct-course` (or `bmad-create-epics-and-stories` in update mode) |
| L-2, L-3, L-8 | Developer dev notes, or `bmad-architecture` when the owning epic starts |
| L-5 | None |

### Resolution (2026-10-19)

Applied after the review, from the binding decisions logged as "Readiness H-1..H-4" in
the architecture `.memlog.md` (H-4 as revised by the user) and the smallest consistent
change for each Medium and Low finding. AD and story numbers are unchanged; no story
was added. Authority order for API shape: spine, then stories, then Usage Skill.

| Finding | Fix |
| --- | --- |
| H-1 `CacheStore` / `DiscordCache` | AD-1 and AD-14 amended: `CacheStore` is the general key-value port, reached through the typed `ctx.kv()` handle; `DiscordCache` is a separate typed port (`update(&Event)`, owned getters, no guards), whose memory default wraps twilight-cache-inmemory, read through `ctx.cache()`. Stories: 3.1 declares `DiscordCache`, 3.2 adds `MemoryDiscordCache`, 7.4 delivers `ctx.kv()`, 12.5 (retitled) wires the cache into core, exposes `ctx.cache()` on every context and adds `.discord_cache_backend(..)`, 14.1 adds a `discord_cache` conformance suite. The template uses `ctx.kv()` for its data and `ctx.cache()` where it called `ctx.discord()`; the skill's `adapters.md`, `commands.md`, `modules-middleware-ratelimits.md`, `modals.md` and `SKILL.md` follow. The PRD already used this split, so it is unchanged. |
| H-2 description source | AD-16 amended: without the `i18n` feature the literal `description = "…"` is the source (missing = CMD-04); with it and a default-locale file, `cmd.<path>.desc` is the source and a differing literal is a compile error; `#[command(.., locales = "<dir>")]` for Module catalogs; `ctxcmd.<key>.name` added; locale files are first read in Epic 11. Story 6.1 (literal only), 6.2 (option descriptions), 11.4 (locale source and conflict fixtures); catalog row CMD-04; skill `commands.md`, `i18n.md`, `troubleshooting.md`, `SKILL.md`. |
| H-3 `RenderJob`, `Template`, `ctx.render` | AD-22 amended: `Template` trait, `RenderJob { template_id, template_version, data (postcard), size, format }`, `TemplateDescriptor`, `Renderer` (`render`, `register`) live in `noctis-ports`; `noctis-image` implements `Renderer`; `ctx.render` calls the port, so core never depends on image. Stories 3.1 (contract), 10.2 (derive implements it, `Module::template` records the descriptor), 10.3 (port call, `Attachment::rendered`). |
| H-4 release-only rule vs trybuild (revised) | Repo-root `.cargo/config.toml` makes `[profile.dev]` release-equivalent (`opt-level = 3`, `debug`, `debug-assertions`, `overflow-checks`, `incremental` all off), which trybuild's cargo runs read; `[profile.release]` stays the real release profile; trybuild suites are `compile_fail` only (`cargo check`), boundary passes are ordinary integration tests in `tests/boundary/`. No exception to the release-only rule. Spine AD-28, Conventions (Builds, Tests), Seed; epics Story conventions, Additional Requirements, Stories 1.1 and 1.7; root `AGENTS.md` (one line saying the dev profile covers trybuild and must not be weakened); catalog §6. |
| M-1 Responder API | AD-11 pins `Responder<K, S = Pending>`, kinds `Command<M>`, `Component<M>`, `ModalSubmit` with `M = NoModal \| OpensModal`, injection as a handler parameter through `CommandResponder<M>`, `ComponentResponder<M>`, `ModalResponder`; `opens_modal` declared on `#[command]` or `#[route]`, alias mismatch is an INT-05 compile error; `followup(msg, Visibility) -> FollowupHandle`, `done() -> Responded`; autocomplete uses only `AutocompleteCtx::respond`. Stories 6.5, 6.6, 6.7, 7.2, 7.8; catalog INT-04, INT-05; skill `responder.md`, `modals.md`, `commands.md`, `troubleshooting.md`, `SKILL.md`; template `edit_bio` takes `ComponentResponder<OpensModal>`. |
| M-2 `#[route]` grammar | AD-6 publishes the grammar: codec arguments (`by_ref`, `overflow = spill`, `schema`, `view`, `select`, `signed`; Story 2.5, which now covers `select`) and runtime arguments (`ttl`, `one_shot` in 7.5; `owner`, `permissions`, `guard` in 7.7; `opens_modal` in 7.8), plus `#[handler]` arguments (`response` 7.2, `on_expired`/`on_stale` 7.3, `rate_limit` 13.4; never on `#[route]`). FR-17's "error" policy is the compile-time rejection of an inline Route; Story 7.5 drops `overflow = error`; PRD FR-17 clarified (memlog `change`). Catalog §2 and skill `routes-payloads-refs.md` aligned. |
| M-3 hidden forward dependencies | Basic nesting (`Module::module(child)`, parent-before-child fold, ancestor paths) moved into Story 5.4, so Story 7.6 needs nothing from Epic 13; Story 13.1 keeps deep trees, unique ids and Module ids in logs. Story 4.1 tests implement `Handled` through the hidden `__private` path the handler macro emits. Story 5.1 no longer mounts Modules. The validation summary lists every "later extended by" note. |
| M-4 Discord test harness | AD-24: `noctis_testkit::fake` (`FakeDiscordHttp` on hyper, `FakeGateway` on tokio-websockets) reached through twilight's HTTP proxy and gateway `proxy_url` (`.gateway_url(..)`), no test-only branch in core; Stack pins added. Built in Story 5.1; Stories 5.2, 5.3, 6.9, 7.8, 8.6, 12.2, 12.6 use it; end-to-end tests live in `crates/noctis-testkit/tests/`. |
| M-5 toolchain pin | `rust-toolchain.toml` pins stable 1.97.0 for development, clippy and UI tests (`.stderr` blessed on it, re-blessed with a bump); CI keeps the MSRV 1.91 `check` job. AD-28, Stack, Story 1.1, Story 1.7, catalog §6. |
| M-6 checks that only fire under `cargo build` | AD-4: compile-time rules must fail under `cargo check` (trait bounds with `#[diagnostic::on_unimplemented]` carrying the rule id, proc macros such as `label!`, free `const _` items); no generic `const` assertion is an enforcement point. Story 4.4 reworded; catalog rows CV2-05, CV2-06, CV2-07, CV2-10, GW-06 and §4/§5 updated. |
| M-7 alt text warning vs error | AD-22: `Attachment::rendered` marks origin `Rendered`. Story 9.1 errors (Strict) only for rendered attachments; Story 4.3 keeps the warning for every other image. |
| M-8 locale and theme files | AD-16: `i18n!(default = "en", dir = "locales")` with both arguments optional, YAML via `yaml-rust2`; AD-15: theme via `cssparser` (MPL-2.0, allowed in `deny.toml`); macros emit `include_bytes!` for every file read. Stack pins; Stories 10.2, 11.1; skill `i18n.md`. |
| M-9 command-line arguments | Conventions row "Command line": built-in parser (no clap), `App::run_with_args(..)`, `noctis::cli::NoctisArgs::split(..)`, unknown flag is a typed error. Story 6.9 (and 12.3, 13.2 join the parser); skill `app-and-sharding.md`. |
| L-1 resolver | `resolver = "3"` in Story 1.1, Additional Requirements and the Seed. |
| L-2 small Story 1.1 choices | Story 1.1 records lints, the `deny.toml` license list (incl. MPL-2.0) and the `xtask` release skip in dev notes, sets `publish = false` on `xtask` and keeps a placeholder in `docs/guides/`; Stories 10.7 and 10.9 record the default-font and Twemoji (CC-BY-4.0) licenses in `NOTICE`. |
| L-3 reference card bench edge | AD-25 and Story 10.11: the bench lives in `noctis-image/benches` (no core needed), reading the same `budgets.toml`; Seed updated. |
| L-4 focused examples | Seed and Story 8.7: `examples/<feature>/` are workspace members with `publish = false`. |
| L-5 technical-layer epics | No change, as the review recommended. |
| L-6 API names no story owns | Story conventions: spelling-only names are settled by the story delivering the behavior; `#[handler(response = …)]` got acceptance criteria in Story 7.2, `ctx.discord()` became `ctx.cache()` (Story 12.5), `Suggestions` was removed in favor of `respond`. |
| L-7 skill API-hash wording | `SKILL.md` status line now says the hash is set when the gate starts and updated with every API change; Story 1.8 states the wording. |
| L-8 `i18n` feature scope | AD-16: the feature gates `i18n!`, public `noctis::i18n` items and locale reading in `#[command]`; the `core → i18n` edge stays unconditional for built-in `noctis.*` strings. |

Files changed: `ARCHITECTURE-SPINE.md` (AD-1, 4, 6, 10, 11, 14, 15, 16, 22, 24, 25,
28, Conventions, Stack, Seed, capability map), architecture `.memlog.md`, `prd.md`
(FR-17) and the PRD `.memlog.md`, `epics.md` (conventions, Additional Requirements,
coverage map, Stories 1.1, 1.7, 1.8, 2.5, 3.1, 3.2, 4.1, 4.3, 4.4, 5.1 to 5.4, 6.1,
6.2, 6.5 to 6.7, 6.9, 7.2 to 7.8, 8.6, 8.7, 9.1, 10.2, 10.3, 10.7, 10.9, 10.11, 11.1,
11.4, 12.2, 12.3, 12.5, 12.6, 13.1, 13.2, 13.4, 14.1), `docs/discord-rules.md`,
root `AGENTS.md`, the Project Template (`src/modules/{profile,dashboard,setup}.rs`)
and its Usage Skill (`SKILL.md` and 9 references). `addendum.md` needed no change.

## Gate 2 (2026-10-19)

### Verdict: FAIL

The bar for this gate is that the team implements all 96 stories back to back without
pausing, so any decision a developer would have to invent counts as a finding. Measured
against that bar, the plan can't be implemented end to end as recorded:

- Story 4.4 depends on a macro that nobody specifies.
- Stories 10.3, 10.7 and 15.4 need a way to wire feature crates into `noctis-core` that the layer graph forbids.
- Story 14.4 can't pass as written, because about 18 catalog rows can never get a fixture of the kind Story 1.7 requires.
- The CI gates only run on pull requests, but the repository workflow never opens one.

Most of the plan is sound. Gate 1's Resolution landed almost everywhere (exceptions are listed below), traceability is complete in both directions, every Stack crate and version exists, and Story 1.1 can be built exactly as written. **Epics 1 to 3 can start now**, provided H-4 is fixed inside Story 1.1. Every other finding must be fixed before its owning epic starts.

### Method

- **Read in full:** the spine, `epics.md` Epics 1 to 3, 5 to 7, 10, 11, 12, 14 and 15, Epics 4, 8 and 9 in substance, `docs/discord-rules.md`, root `AGENTS.md`, and the template's `Cargo.toml`, `lib.rs`, `main.rs`, `AGENTS.md`, `locales/en.yaml` and its Usage Skill (`SKILL.md`, `images.md`, `routes-payloads-refs.md`, `i18n.md`, `adapters.md`, `responder.md`, `modals.md`, `app-and-sharding.md`). I also read the PRD FR-17 and cache sections, the addendum's pins, and `reviews/review-versions.md`.
- **Traceability, by script:**
  - FR-1..FR-79, NFR-1..NFR-10 and AD-1..AD-29 all appear in story `Implements:` lines.
  - All 123 catalog rule ids are cited by a story, with ranges expanded, and no story cites an unknown id.
  - Every story traces back to an FR, NFR, AD or rule id.
  - Forward story references were scanned across the whole story order. They are all "later extended by" notes, except the ones in H-1, M-2 and M-7.
- **crates.io:** queried the API with User-Agent `noctis-readiness-check` for every Stack row. All exist:

  | Crate | Version found | `rust-version` |
  | --- | --- | --- |
  | tokio | 1.53.3 | |
  | twilight-* (5 crates) | 0.17.1 | 1.89 |
  | takumi | 2.17.0 | 1.91 |
  | syn | 3.0.9 | |
  | getrandom | 0.4.3 | |
  | regex | 1.13.1 | |
  | trybuild | 1.0.122 | 1.88 |
  | insta | 1.49.0 | |
  | proptest | 1.12.0 | 1.88 |
  | hyper | 1.12.0 | |
  | hyper-util | 0.1.21 | |
  | http-body-util | 0.1.5 | |
  | tokio-websockets | 0.13.3 | |
  | yaml-rust2 | 0.13.0 | 1.85 |
  | cssparser | 0.38.0 | 1.85 |
  | criterion | 0.8.2 | |
  | dhat | 0.3.3 | |
  | cargo-public-api | 0.52.0 | |
  | cargo-semver-checks | 0.51.0 | |
  | cargo-deny | 0.20.2 | |
  | release-plz | 0.3.172 | |

  - Licenses: yaml-rust2 is MIT OR Apache-2.0. cssparser is MPL-2.0, as are its dependencies `cssparser-macros`, `dtoa-short` and takumi's `selectors`; Story 1.1 allows MPL-2.0 in `deny.toml`.
  - MSRV: no Stack crate needs more than 1.91. takumi needs exactly 1.91.
  - Crate names: `noctis` and all eleven `noctis-*` names are still free.
- **Toolchains:**
  - Rust 1.97.0, 1.91.0 and `nightly-2026-10-15` all exist on static.rust-lang.org.
  - The pinned nightly emits rustdoc JSON `format_version` 61, but `public-api` 0.52.2 is built against `rustdoc-types` 0.59.
  - I tested this: `public-api` 0.52.2 parsed the nightly's JSON for a non-trivial test crate without error. The `api-snapshot` pin therefore works today. Story 1.8 should still fail loudly if a future nightly breaks it.
- **trybuild 1.0.122, checked in its source:**
  - It runs cargo with the current directory set to `target/tests/trybuild/<crate>/`, which is under the repository root, so the repo-root `.cargo/config.toml` dev profile applies.
  - It never copies `[profile]` sections.
  - With only compile-fail cases it runs `cargo check`.
  - It passes `--no-default-features --features <the features enabled for the test>`. This last point matters for M-3.

### Gate 1 resolutions: did they land?

| Gate 1 finding | Status in Gate 2 |
| --- | --- |
| H-1 `CacheStore` / `DiscordCache` | Landed in the spine, Stories 3.1, 3.2, 7.4, 12.5 and 14.1, the PRD, the template and the skill. |
| H-2 description source | Landed in AD-16, Stories 6.1, 6.2 and 11.4, the catalog's CMD-04 row and the skill. The locale *file naming* gap is new (M-5). |
| H-3 render contract | Landed in AD-22 and Stories 3.1, 10.2 and 10.3. The contract it froze can't express FR-69 (M-1). |
| H-4 release-only rule vs trybuild | Landed, and verified in trybuild's source as above. |
| M-1 Responder API | Landed in AD-11, the catalog's INT-04 and INT-05 rows, Stories 6.5, 6.6, 7.2 and 7.8, `responder.md`, `modals.md` and the template's `edit_bio`. |
| M-2 `#[route]` grammar | Landed in AD-6, Stories 2.5, 7.5, 7.7, 7.8 and 13.4, PRD FR-17 and `routes-payloads-refs.md`. |
| M-3 hidden forward dependencies | Landed in Stories 5.4, 4.1 and 5.1 and in the validation summary. |
| M-4 Discord test harness | Landed in AD-24, the Stack and Story 5.1. Story 5.1 is now oversized (M-7). |
| M-5 toolchain pin | Landed. |
| M-6 checks that fire only under `cargo build` | **Partly landed.** The CV2-03 row still says ``const { assert!(T::COST <= 40) }` in `view!``, which contradicts AD-4 and the catalog's own §5. Story 1.6 still says rule functions are `const fn` "so later builders can assert literals with `const { … }`", a mechanism AD-4 doesn't list. See H-2 and L-2. |
| M-7 alt text warning vs error | Landed in Stories 4.3 and 9.1. It introduced a layer violation (M-2). |
| M-8 locale and theme files | Landed: yaml-rust2 and cssparser are pinned and verified. |
| M-9 command-line arguments | Landed. |
| L-1 to L-8 | Landed. One exception: the Story 7.4 / skill spelling clash on sharing (L-3). |

### Findings (ordered by severity)

#### H-1 No recorded way to wire the feature crates into `noctis-core`

**Where:**
- **Spine:**
  - AD-24 says "Process-scoped (owned by `noctis::Runtime`, which Apps attach to)", covering the render pool, takumi caches, fonts, the emoji font and the metrics recorder.
  - The Seed lists `Runtime` in the facade, while the layer table says the facade has "none (re-exports)".
- **Stories:**
  - Story 5.1 needs `noctis::Runtime`.
  - Story 10.3 says that with the `image` feature, "noctis-image implements the `Renderer` port", implying automatic wiring.
  - Stories 10.7 (process-scoped font registry), 10.2 ("fails the test under the testkit") and 10.11 (testkit pixel snapshots) all need wiring too.
  - Story 15.4 has `noctis-prefix` running pipeline stages 3 to 10 on `MESSAGE_CREATE`.
- **Usage Skill and template:**
  - `app-and-sharding.md` lines 43, 44 and 221 use `.theme(..)`, `.image(ImageConfig)` and `Runtime::builder().image(..)`.
  - The template's `lib.rs` calls `.theme(noctis::image::theme!())` on core's `AppBuilder`.

**Problem:** core can't depend on `noctis-image`, `noctis-prefix` or the facade, and `noctis-testkit` has no edge to `noctis-image`. Nothing records:
- where `Runtime` is defined;
- how a facade feature installs the takumi Renderer, Theme and fonts on core's builder;
- how core hands messages to `noctis-prefix`;
- how the testkit reaches the image stack.

Each developer of Stories 5.1, 10.3, 10.7, 10.11 and 15.4 would invent a different mechanism.

**Fix (`bmad-architecture`, then `bmad-correct-course`):** add a decision to AD-2 or AD-24 that covers:
- `Runtime` lives in core and holds type-erased process resources, and the facade re-exports it;
- core's `AppBuilder` exposes `renderer(Arc<dyn Renderer>)` and an opaque `theme(ThemeData)` from `noctis-ports`;
- the facade adds feature-gated extension traits (for example `noctis::image::AppBuilderExt::image(ImageConfig)`) that install the defaults;
- a core message-command hook that `noctis-prefix` registers through;
- either a feature-gated `testkit → image` edge, or the pixel-snapshot helpers moving into `noctis-image` behind `testkit`.

Update the Invariants graph and `deps-check` to match, and amend Stories 5.1, 10.2, 10.3, 10.7, 10.11 and 15.4 and the skill.

#### H-2 The static V2 component-tree macro is unspecified, and its name collides with the image `view!`

**Where:**
- Story 4.4 ("the free `const _ … assert_budget::<V>()` item that **the static tree macro** emits").
- Catalog CV2-03 (`const { assert!(T::COST <= 40) }` in `view!`), CV2-04 ("literals summed in the macro"), CID-02 (Compile-time: "the `view!` macro rejects two identical (Route, literal Payload) pairs") and Compile-time strategy §5.
- In the spine, AD-15 and the Seed use `view!` for the **image Template** DSL (`noctis::image::view!`), and the skill's `images.md` does the same.

**Problem:**
- No story or AD names, specifies or delivers a static message-tree macro.
- CID-02's Compile-time tier has no owning story, so `rules-catalog` can never clear it.
- The CV2-03 row still prescribes a generic `const` assertion, which AD-4 forbids.

**Fix:**
1. With `bmad-architecture`, either specify the macro in AD-4 or AD-12: its name (for example `message!{}`), its syntax, the `assert_budget` it emits and its CID-02 duplicate check. Or remove the static-tree tier, so that CV2-03, CV2-04 and CID-02 become Runtime only, except for typed tuple costs.
2. With `bmad-correct-course`, make Story 4.4 own the macro and the CID-02 fixture, and rewrite the catalog's CV2-03, CV2-04 and CID-02 rows.

#### H-3 The rules-catalog gate is unsatisfiable for by-construction and missing-API rows

**Where:**
- Story 1.7 says every Compile-time row needs a trybuild fixture whose `.stderr` contains the rule id.
- Story 14.4 says `rules-pending.toml` must be empty, and allows only CMD-07, REG-06, CMD-14 and LOC-04 as documentation-only rows.
- Story 6.5 asks for INT-04 fixtures ("`CommandResponder` has no `update`").

**Problem:** for the rows below, the compile failure is either impossible or a plain rustc error (E0599 or E0609, "no method/field named …") that never prints `[ID]`:

| Group | Rows |
| --- | --- |
| By construction, with no failing input at all | CV2-01, CV2-14, CID-07, INT-08, MSG-05, WH-01, META-05, OPT-14, CTX-02 |
| The API is absent, so rustc's error carries no rule id | CV2-02, INT-04, INT-06, MSG-03, WH-03 |
| Typed or fixed by construction | CTX-03, CID-05, INT-10, ATT-07 |
| No API exists for the rule to apply to | MSG-02 (`LegacyMessage` is plain text only, Story 4.2) |

Story 14.4 can't pass as written. The developer would invent either a convention or exemptions.

**Fix (`bmad-correct-course` on Stories 1.7 and 14.4 and the catalog):**
- Add an Enforcement value such as `Compile-time (by construction)`, which `rules-catalog` accepts with a unit or doc test citing the id. Alternatively, allow the id as a `// [ID]` comment on the offending line, so it appears in the `.stderr` snippet.
- List the affected rows explicitly.
- Decide MSG-02: either drop its Compile-time tier, or add an embed builder to `LegacyMessage`.

#### H-4 The CI gate model conflicts with the repository workflow

**Where:**
- Root `AGENTS.md`, Maintainer workflow: "Push every commit to `main` immediately … no PRs".
- These rely on pull requests:
  - Stories 1.1, 1.2, 1.7 and 1.8 ("Given a pull request … required job");
  - AD-28 ("Required PR gates");
  - release-plz's "release PR … on merge" (AD-28, Story 14.7).
- The Story conventions say "Done means … every CI gate that exists at that point".

**Problem:**
- With no pull requests, `ci.yml` as specified never runs, so "required" gates gate nothing.
- If branch protection enforced the required checks, every direct push to `main` would be rejected.
- Stories 14.5 (five agent trials) and 14.7 (merging the release PR, the protected-environment crates.io publish) are maintainer actions, which an uninterrupted run can't complete.

**Fix (`bmad-correct-course` on Story 1.1, the Story conventions and Stories 14.5 and 14.7; `bmad-project-context` for `AGENTS.md`):**
- `ci.yml` runs on `push` to `main` as well as `pull_request`.
- Done means the local `--release` commands pass and CI is green on the pushed commit.
- Record 14.5 and 14.7 as maintainer-gated stories, or move them out of the unattended run, with the agent's part ending at "release PR opened" and "trial harness ready".

#### M-1 The render contract can't express FR-69's per-frame animation, and `Template` can't be decoded

**Where:**
- AD-22: `RenderJob { template_id, template_version, data: Bytes, size, format }`, and `trait Template: Serialize`.
- Story 3.1, which freezes these ("Stories 10.2 and 10.3 build on exactly these types").
- Story 10.6, which asks for "one Template state per frame".
- The skill's `images.md`: `ctx.render_frames(Frames::new(..))` [planned], and the `ProfileCard` example that derives only `Serialize`.

**Problem:**
- A single `data` can't carry N frame states.
- The Renderer must postcard-*decode* the data to call `TemplateView::view(&self)`, which needs `DeserializeOwned`.
- `ErasedView` and `TemplateView` are only named; nothing defines them.

**Fix:**
1. `bmad-architecture` amends AD-22:
   - frames, as `data: RenderData::{One(Bytes), Frames(Vec<Bytes>)}` or as a separate field;
   - `Template: Serialize + DeserializeOwned`;
   - `ErasedView` as a decoder (`fn(&[u8]) -> Result<Node, RenderError>`);
   - `ctx.render_frames`.
2. `bmad-correct-course` updates Stories 3.1 and 10.6 and the skill.

#### M-2 Two types sit in the wrong layer

**Where and problem:**
- **`Attachment::rendered(RenderOutput)`.** AD-22 and Stories 9.1 and 10.3 put this constructor in `noctis-ui`, but `RenderOutput` lives in `noctis-ports`, and the Invariants graph has no `ui → ports` edge.
- **`RawJson`.** AD-10 defines it in `noctis-gateway`, which is layer 3, yet Story 4.6 decodes modal submissions "from raw JSON (`RawJson`)" inside `noctis-ui`/`noctis-macros` in Epic 4, before Epic 5 creates the gateway types. That is a forward dependency plus a forbidden edge.

**Fix (`bmad-architecture`):**
- Make `Attachment::rendered` take primitives (bytes, digest and extension), or add a `ui → ports` edge.
- Define `RawJson` in `noctis-codec` or `noctis-ui` (or decode from `&[u8]`), with the gateway re-using it.

Then amend AD-10, AD-22 and Story 4.6.

#### M-3 Feature-gated tests and fixtures never run correctly

**Where:**
- Story 1.7 has one `compile_fail("tests/ui/**/*.rs")` in the facade.
- Story 1.1 CI, the Story conventions and `AGENTS.md` run `cargo test --release` without `--all-features`. The feature matrix only *builds*.

**Problem:**
- trybuild builds fixtures with exactly the features the test run enabled.
- So these fixture directories fail for the wrong reason, or their `.stderr` drifts by feature set:
  - `tests/ui/tw/` (`image`);
  - `tests/ui/loc/` and the Story 11.4 locale fixtures (`i18n`);
  - prefix fixtures.
- Pixel tests, i18n runtime tests and image unit tests never run in CI.
- The default features of `noctis-image` (is `render` on?) are undefined.

**Fix (`bmad-correct-course`, Stories 1.1 and 1.7):**
- CI runs `cargo test --release --workspace --all-features` alongside the default run.
- Fixture directories are gated per feature with `#[cfg(feature = "…")]`.
- `.stderr` files are blessed under one stated feature set.
- Record `noctis-image`'s default features.

#### M-4 The TLS and crypto-provider stack is unpinned

**Where:**
- AD-21 and Story 5.1: "installs a rustls crypto provider if none is installed".
- The Stack.

**Problem:**
- Neither `rustls` (0.23.x on crates.io) nor the choice of provider (ring or aws-lc-rs) is recorded. aws-lc-rs needs cmake and clang on some targets.
- twilight's TLS feature set isn't recorded either. Its defaults are `rustls-platform-verifier` plus `zstd` on the gateway, and `rustls-platform-verifier` plus `decompression` on HTTP.

**Fix (`bmad-architecture` Stack):** add `rustls 0.23` with the chosen provider feature and the twilight feature list. Story 5.1 adds them to `[workspace.dependencies]`.

#### M-5 Locale file stems and command-key spelling are undefined

**Where:**
- AD-16 says files are "named by Discord locale code", with `default = "en"`.
- LOC-01, and Story 11.1's fixture, reject unknown file names.
- FR-41, the skill's `i18n.md` and the template's `en.yaml` rely on base-language files (`en` covers en-US and en-GB; `es`, `pt`, `zh`).
- The template keys `cmd:` → `setup start:` (one YAML key containing a space).
- `i18n!` makes `keys::` consts with "dots → `_`, uppercased".

**Problem:**
- `en` isn't one of the 32 Discord codes. No type or list of accepted base languages exists (Story 1.3 only maps regional codes to "their base language").
- The spelling of `<path>` inside `cmd.<path>.*` isn't recorded. A space-containing key breaks the generated const and accessor names.

**Fix (`bmad-architecture` AD-16, then `bmad-correct-course` on Stories 1.3, 11.1 and 11.4):**
- List the accepted file stems: the 32 codes plus `en`, `es`, `pt`, `zh`, `sv`.
- Name the type that represents a base language.
- Fix the `<path>` spelling (a single YAML key joined by spaces, or nested).
- State that `cmd.*` and `ctxcmd.*` keys get no accessors, or sanitized ones.

#### M-6 Story 10.1 is still a design spike, and the takumi pin doesn't freeze the parser

**Where:**
- AD-15 and Story 10.1 ("`xtask tw-grammar` generates it from takumi's source"; "`tw-crosscheck` … fails on any drift").
- `reviews/review-versions.md` F1: takumi accepts any `<known-prefix>-<word>`, and its parsers are `pub(crate)`.
- On crates.io, `takumi` 2.17.0 is a thin facade that depends on `takumi-core ^0.31.0`.

**Problem:**
- Nothing records how the grammar is extracted: which source files, and with what parser.
- Nothing records the oracle `tw-crosscheck` uses to decide that a class "renders". Layout-only classes produce no visible pixels on their own.
- `takumi = "=2.17.0"` doesn't freeze `takumi-core` 0.31.x. With no `Cargo.lock` policy recorded, grammar and pixel drift can arrive without a takumi bump.
- Gate 1 suggested a spike for this; it wasn't added.

**Fix:**
1. `bmad-architecture` (AD-15 and AD-28) records:
   - the extraction method (for example, syn-parsing `takumi-core/src/style/tw/*.rs` at the pinned version);
   - the crosscheck oracle (for example, comparing the computed style or layout with and without the class);
   - `takumi-core = "=0.31.x"`;
   - that `Cargo.lock` is committed.
2. `bmad-correct-course` adds a spike story before Story 10.1.

#### M-7 Story 5.1 is oversized

**Where:** Story 5.1.

**Problem:** one story covers the App builder, `from_env`, the token newtype, the runtime and crypto checks, the HTTP client, `Runtime` (see H-1), **and** both Discord fakes. The fakes are a hyper HTTP server, and a websocket gateway speaking HELLO, IDENTIFY/READY, RESUME, heartbeats, dispatches and close codes. Every end-to-end story from 5.2 on blocks on it.

**Fix (`bmad-correct-course`):** split the fakes into their own story, placed before Story 5.2.

#### L-1 The `deny.toml` license list is open-ended

**Where:** Story 1.1, "MIT, Apache-2.0 and the other licenses of the Stack".

**Problem:** I resolved the full Stack tree with `cargo metadata` (takumi tree included). Besides MIT, Apache-2.0 and MPL-2.0, it needs:
- Unicode-3.0, from the icu crates and `unicode-ident`;
- ISC, from twilight and rustls;
- BSD-2-Clause and BSD-3-Clause;
- Zlib;
- CDLA-Permissive-2.0, from `webpki-root-certs`;
- BSL-1.0, from `xxhash-rust`;
- IJG, from `jpeg-encoder`;
- CC0-1.0;
- Apache-2.0 WITH LLVM-exception.

The workspace crates themselves also need `license = "MIT OR Apache-2.0"` (from the README) in `[workspace.package]`, or `cargo deny` flags them as unlicensed.

**Fix:** list these in Story 1.1.

#### L-2 Leftovers of the generic-`const` mechanism

**Where:**
- Story 1.6 ("`const fn` so later builders can assert literals with `const { … }`").
- The catalog's CV2-13 row (`Rgb(u32)` "checks range in `const fn`").
- Story 4.4 uses `rgb!` instead.

**Fix:** reword these to the AD-4 mechanisms.

#### L-3 Two spellings for opting a port into sharing

**Where:** Story 7.4 says `.share(..)`; the skill's `app-and-sharding.md` line 231 says `.cache_store(Shared::new(store))`.

**Fix:** pick one in Story 7.4.

#### L-4 The takumi pin differs between artifacts

**Where:** the PRD addendum says "pin 2.17.x"; the spine says `=2.17.0`.

**Problem:** an exact pin in a published library also blocks bot authors from taking takumi patches (`review-versions` F3).

**Fix:** decide in AD-15 and make the addendum match.

#### L-5 Supporting crates are unpinned

**Where:** none of these is in the Stack:
- `proc-macro2`;
- the parsers `xtask` needs: TOML (`rules-pending.toml`, `budgets.toml`), Markdown tables (the catalog) and `cargo_metadata`;
- a PNG decoder for pixel snapshots.

**Fix:** add them to the Stack, or record them in the dev notes of Stories 1.2, 1.7 and 10.11.

#### L-6 How to measure Discord-cache memory isn't recorded

**Where:** Story 12.6 reports "Discord cache bytes" separately.

**Problem:** twilight-cache-inmemory exposes no size.

**Fix:** record the method, for example the allocation delta between runs with the cache on and with it off.

### Epic 1 Story 1: can it be built exactly as written?

Yes, once H-4 (the CI trigger) and L-1 (licenses) are folded in.

| Item | Status |
| --- | --- |
| Virtual workspace with `resolver = "3"` (cargo ≥ 1.84; MSRV 1.91 is fine), 12 crates plus `xtask`, `exclude = ["examples/project-template"]` | Buildable |
| `rust-toolchain.toml` 1.97.0 with rustfmt and clippy | The toolchain exists |
| Repo-root `.cargo/config.toml` with a release-equivalent `[profile.dev]` | Valid cargo config keys. trybuild's runs pick it up, verified in trybuild's source. |
| Template excluded | Consistent everywhere: the template has its own `[workspace]` and a path dependency on the facade's empty features. |
| CI jobs | Specified. They are triggered only by `pull_request` (H-4), and `cargo test` doesn't use `--all-features` (M-3). |
| MSRV job `cargo +1.91 check --release --workspace` | Every Stack dependency has `rust-version ≤ 1.91` |
| Release-only commands | Consistent in the spine, the Story conventions, both `AGENTS.md` files and the catalog's §6 |
| `deny.toml` | Open-ended license list (L-1) |

### Fix routing

| Finding | Skill | Must land before |
| --- | --- | --- |
| H-4, L-1 | `bmad-correct-course` (Story 1.1, Story conventions, Stories 14.5 and 14.7), `bmad-project-context` (`AGENTS.md`) | Story 1.1 |
| H-3, M-3 | `bmad-correct-course` (Stories 1.7 and 14.4, the catalog) | Story 1.7 |
| M-1, M-2 | `bmad-architecture` (AD-10, AD-22), then `bmad-correct-course` (Stories 3.1, 4.6, 9.1, 10.6) | Story 3.1 |
| H-2, L-2 | `bmad-architecture` (AD-4 or AD-12), then `bmad-correct-course` (Stories 1.6 and 4.4, the catalog) | Epic 4 |
| H-1, M-4, M-7 | `bmad-architecture` (AD-2, AD-21, AD-24, Stack, Invariants), then `bmad-correct-course` (Stories 5.1, 10.x, 15.4, the skill, the template) | Epic 5 |
| M-5 | `bmad-architecture` (AD-16), then `bmad-correct-course` (Stories 1.3, 11.1, 11.4) | Story 1.3 |
| M-6, L-4, L-5 | `bmad-architecture` (AD-15, AD-28, Stack), then `bmad-correct-course` (spike before Story 10.1) | Epic 10 |
| L-3, L-6 | `bmad-correct-course` | the owning story |
