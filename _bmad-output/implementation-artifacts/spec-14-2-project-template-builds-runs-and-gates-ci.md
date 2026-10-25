---
title: 'Story 14.2: Project Template builds, runs and gates CI'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_revision: 'cefed1f2d569f993d13310a92ee3887345fe0dfc'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/tooling-and-project-layout.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/serde.md'
warnings: ['oversized']
deferred:
  - summary: >-
      twilight `Id<T>` cannot round-trip through `Kv` (postcard is not self-describing; twilight's `Deserialize` calls `deserialize_any`), so any bot storing a struct with an `Id` field reads `PortError::Value(Malformed)`.
    evidence: |-
      Planning (2026-10-25): the template's `StoredWebhook { id: Id<WebhookMarker>, .. }` broke `setup_finish_logs_through_its_webhook` with `Value(Malformed)`. This story fixes the template (u64 fields) and documents the trap; a framework-level guard (serde helper or put-time rejection) is left to Story 14.4/14.5.
    location: crates/noctis-core/src/kv.rs; examples/project-template/.claude/skills/noctis/references/adapters.md §3
    severity: medium
  - summary: >-
      The Usage Skill still lists a `[planned]` shared-accessor trait `Ctx` (references/commands.md ~line 917); the template no longer uses it and no such trait exists.
    evidence: |-
      This story removes `impl Ctx` from the template instead of adding the trait (Design Notes D2). Story 14.5 (removes every `[planned]` marker) must delete or deliver that line.
    location: examples/project-template/.claude/skills/noctis/references/commands.md
    severity: low
  - summary: >-
      The template's `post_log` never deletes the old log webhook in Discord when the log channel changes (or the stored ids are invalid); repeated channel switches leave orphaned webhooks toward Discord's per-channel cap.
    evidence: |-
      Review 14.2 pass 1 (blind, edge): pre-existing behaviour (baseline `post_log`/`create_log_webhook` already overwrote the stored record without deleting the old webhook); this story only changed the id types. Fix: delete the stale owned webhook (ignore 404) before creating the replacement.
    location: >-
      examples/project-template/src/modules/setup/handlers.rs post_log / create_log_webhook
    severity: low
  - summary: >-
      Nothing tells maintainers or story agents to refresh `examples/project-template/Cargo.lock` when a workspace crate's version or dependencies change, although every template gate is now `--locked`.
    evidence: |-
      Review 14.2 pass 1 (blind, intent): a dependency bump in the workspace changes the template's resolve and `template-build` fails with cargo's "lock file needs to be updated" error. The fix edits agent-context files (root `AGENTS.md` policy next to the template rule, or `gates.sh` comments): add `cargo update --manifest-path examples/project-template/Cargo.toml -p noctis` (or `--workspace`) to the dependency-change policy.
    location: >-
      AGENTS.md (Policy / Running and verifying); tools/story-loop/gates.sh
    severity: low
  - summary: >-
      The Usage Skill's new Kv id note (references/adapters.md §3) does not say what to do with a value already stored with `Id<_>` fields (it reads back as `Malformed`: treat as absent and overwrite).
    evidence: |-
      Review 14.2 pass 1 (blind): one sentence is missing for bots that stored `Id`-typed structs before the note; the fix edits the Usage Skill (agent context), and Story 14.5 owns skill completeness.
    location: >-
      examples/project-template/.claude/skills/noctis/references/adapters.md §3
    severity: low
---

<intent-contract>

## Intent

**Problem:** `examples/project-template/` does not compile: its lockfile drifted to `takumi-raster 0.5.10` (needs `takumi-core 0.33`, while `takumi 2.17.0` pins `0.31`), and its sources still use planned API (`noctis::id`, a prelude `Id`, a `Ctx` trait). Its CI job is tracking-only, so nothing caught this, and two flow tests fail on real behaviour (an `Id` stored in `Kv` cannot be read back; an ephemeral message clicked by another user).

**Approach:** Pin `takumi-raster` exactly in `noctis-image`, add the small `noctis::id` / prelude `Id` re-export the template and skill already document, fix the template until build, clippy (`-D warnings`), fmt and tests pass (default and all features), commit its snapshots, remove the "planned API" notes, make `template-build` a required gate in `ci.yml` and in the local gates, and report the SM-1 line count.

## Boundaries & Constraints

**Always:**
- Public API change (exact): `crates/noctis/src/lib.rs` adds `pub use twilight_model::id;` at the crate root with a doc comment (so `noctis::id::Id`, `noctis::id::marker::*`), and `pub use twilight_model::id::Id;` in `pub mod prelude`. Same commit: template code, template `AGENTS.md`, Usage Skill, `xtask/public-api.txt` re-blessed, `SKILL.md` `API hash` line updated.
- Dependency pin: workspace `Cargo.toml` gets `takumi-raster = { version = "=0.5.8", default-features = false }` next to `takumi`/`takumi-core` with a comment (takumi 2.17.0 depends on `takumi-raster ^0.5.8`; 0.5.9+ require newer `takumi-core`; remove the pin with the next takumi bump). `crates/noctis-image/Cargo.toml` adds `takumi-raster = { workspace = true, optional = true }` and `"dep:takumi-raster"` in the `render` feature (deps-check `takumi-gated` requires optional + render-only). No new crate enters any graph (0.5.8 is already in the root `Cargo.lock`).
- The template stays a standalone workspace with `noctis` as a path dependency; its `Cargo.lock` is committed and every gate command uses `--locked`.
- Template gate commands (run from the repo root, all with `--manifest-path examples/project-template/Cargo.toml`): `cargo fmt --check`; `cargo build --release --locked`; `cargo clippy --release --locked --all-targets -- -D warnings`; `cargo clippy --release --locked --all-targets --all-features -- -D warnings`; `CI=true cargo test --release --locked`; `CI=true cargo test --release --locked --all-features`. With `CI=true` a missing or changed insta / pixel snapshot fails instead of being written.
- Every value type the template writes with `ctx.kv().put` holds Discord ids as `u64` (`Id::get()`), never `Id<_>`; reading back rebuilds ids with `Id::new_checked` (a `0` counts as no stored value). No `unwrap`/`expect`/panicking `Id::new` on stored data.
- Handlers keep the UI Doctrine: no embeds or plain `content` as primary output, no manual response-type selection, no collectors/await loops/inline listeners/spawn (`xtask skill-check` doctrine lint stays green).
- Template edits follow its own `AGENTS.md` and clippy config (`pedantic`, `unwrap_used`/`expect_used` warn, tests may unwrap).

**Never:**
- No new shared context trait (`Ctx`) or other new framework API beyond the two `id` re-exports; restructure template helpers instead.
- No weakening of a template test to make it pass (a test may change only where it asserted behaviour Discord itself forbids, see the matrix); no `#[ignore]`, no lint `allow` without a `reason`.
- No changes to `.cargo/config.toml`, `rust-toolchain.toml`, `deny.toml`; no `cargo hack`; no `[planned]` cleanup in the Usage Skill beyond the lines this story names (Story 14.5 owns the rest).
- Do not move workflows out of `.github/workflows-disabled/`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Fresh resolve | template lock regenerated after the pin | lock has `takumi-raster 0.5.8`, one `takumi-core` (0.31.0); `cargo tree --locked -i takumi-core` shows one version | `--locked` fails on any drift |
| Prelude `Id` | `use noctis::prelude::*;` plus explicit `use noctis::twilight::model::id::Id;` | compiles (explicit import shadows the glob; same item) | — |
| `noctis::id` path | `noctis::id::marker::GuildMarker`, `noctis::id::Id::<GuildMarker>::new(1)` | same types as `noctis::twilight::model::id::*` | — |
| Setup step 1 | `/setup start` by ADMIN in GUILD | ephemeral Reply "Step 1 of 3: Log channel", Next disabled; JSON equals committed `tests/snapshots/flows__setup_step1.snap` | — |
| Finish twice | two full flows, fake webhook routes | 1 webhook create, 2 sends, both `wait=true&with_components=true`, flags 32768, no `content` | — |
| Stored webhook, other channel | stored `channel` ≠ configured log channel | new webhook created and stored, line sent through it | — |
| Stored webhook deleted | send answers 404 | key deleted, one new webhook created and used | other errors: `tracing::warn!`, setup still answers "Setup complete" |
| Stored id 0 | stored `StoredWebhook.id == 0` | treated as no stored webhook (create new) | — |
| Undecodable stored entry | bytes that do not decode | `PortError::Value` → `warn!`, log not posted, setup still succeeds (existing behaviour) | no panic |
| Kv value round trip | `GuildConfig { log_channel: 300, mod_role: 400 }`, `StoredWebhook { id: 900, token: "t", channel: 300 }`, and a token of 0 bytes / 200 non-ASCII chars (`"ş".repeat(200)`) | `decode_value(0, &encode_value(0, &v)?)` == `Ok(Some(v))` (the codec `Kv` uses) | — |
| Ephemeral, other user | MEMBER clicks Cancel on ADMIN's ephemeral setup message | `Err(TestError::NotVisible { user: 201 })` (Discord shows ephemeral messages only to their user) | — |
| Owner guard | MEMBER runs `/profile`; ADMIN clicks its `EditBio` | ephemeral guard notice (`is_guard_notice()`), handler not run | — |
| Profile outside guild | `guild_id()` is `None` | `HandlerError::user(keys::COMMON_GUILD_ONLY)` | — |
| Profile, uncached user | cache has no user | name = user id string, placeholder avatar | — |
| Pixel snapshot | `profile_card` render vs committed `tests/snapshots/profile_card.png` | passes at `tolerance = 0.01`; with `CI=true` a missing PNG fails | mismatch writes `*.new.png` (gitignored) |
| Voice / prefix | `--all-features` | lib, tests (`tests/voice.rs`) build, clippy clean, tests pass | — |
| SM-1 count | the awk command below over the SM-1 files | numbers recorded in template `README.md` and the Auto Run Result, compared to ≤ 150; a miss is reported, not hidden | — |

</intent-contract>

## Code Map

- `Cargo.toml` (root) l.59-60 -- `takumi =2.17.0`, `takumi-core =0.31.0`; add the `takumi-raster` pin below.
- `crates/noctis-image/Cargo.toml` l.14-34 (`render` feature), l.86-94 (takumi deps) -- add optional `takumi-raster`.
- `xtask/src/deps_check.rs` l.1128-1150 `check_takumi` -- read-only; confirms the new dep's shape is allowed.
- `crates/noctis/src/lib.rs` l.2018-2035 `pub mod twilight` (precedent: `pub use twilight_model as model;`), l.2534-2566 `pub mod prelude` -- add the two re-exports.
- Template build failures found in planning (after the pin): unresolved `noctis::id` (`src/modules/{setup,dashboard,profile}/*.rs`), `Id` (expected from the prelude), `Ctx` (`src/modules/dashboard/handlers.rs:120` `load`, `src/modules/profile/handlers.rs:202` `load`, `:235` `guild_of`); `src/modules/profile/handlers.rs:224` `p.label(locale)` (Choices `label()` takes no locale and returns `&'static str`); `src/modules/setup/view.rs:111-112` `map_or_else` arms `&str` vs `String`.
- Context accessors that exist on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` as inherent methods: `guild_id()`, `kv() -> noctis::Kv`, `cache() -> noctis::DiscordCacheReader`, `modules() -> noctis::Modules`, `response_locale()`, `render(t)` (`crates/noctis-core/src/{command.rs,component.rs}`, `responder/ergonomics.rs:522`). Restructure: dashboard `load(cache, modules, guild, meta)`; profile splits the I/O into a helper taking `(kv, cache, locale, guild, user)` that returns the card data, and each handler calls `ctx.render(card).await?` itself; `guild_of(ctx.guild_id())`.
- Clippy `-D warnings` hits found in planning (lib only; tests may add more): `unused_qualifications` `src/modules/profile/mod.rs:29,43`; `doc_markdown` `src/modules/profile/events.rs:14,16`; `collapsible_if` `src/modules/setup/handlers.rs:162`; `sort_unstable_by_key` `src/modules/dashboard/handlers.rs:125`; `duration_suboptimal_units` `src/modules/profile/events.rs:32`. `cargo fmt --check` fails on `src/bootstrap.rs`.
- `src/modules/setup/mod.rs:141-163` `GuildConfig`, `StoredWebhook` (Id fields → `u64`); `src/modules/setup/handlers.rs:130-196` `FinishSetup`, `post_log`, `create_log_webhook`.
- `crates/noctis-ports/src/value.rs` `ValueError::Malformed`; `crates/noctis-core/src/kv.rs:92` `Kv::get` -- read-only, why `Id` fails.
- `tests/flows.rs:155-163` `setup_rejects_other_users` (ephemeral → `NotVisible`, `crates/noctis-testkit/src/app/component.rs:28`); `:56` insta snapshot; `tests/templates.rs` pixel snapshot (`assert_render_snapshot!`, CI flag in `crates/noctis-testkit/src/snapshot.rs:130`).
- "planned API" notes: line 1 of every file under `src/` and `tests/` that has one (`src/{lib,main,strings}.rs`, `src/adapters/{mod,bounded_cache}.rs`, `src/modules/**`, `tests/{adapters,flows,memory,templates}.rs`, incl. `tests/templates.rs:2`), `src/modules/profile/mod.rs:80` `[planned]`, `README.md:7-8` Status block.
- `.github/workflows-disabled/ci.yml` l.30-32 (build-config comment), l.202-218 `template-build` job.
- `tools/story-loop/gates.sh` -- local gates; add template steps after `test`.
- `AGENTS.md` (root) "Running and verifying" -- gate list.
- Template docs: `examples/project-template/AGENTS.md` l.30 (commands), l.67 (webhook storage); `README.md`; `.gitignore`.
- Skill: `references/adapters.md` §3 (Kv, after "Values are postcard…" ~l.572) and l.340-343 (stale "noctis does not re-export it yet [planned]"); `references/routes-payloads-refs.md:18` already uses `noctis::id`; `SKILL.md:85` lists `noctis::id`, `SKILL.md` `API hash` line.

## Tasks & Acceptance

**Execution:**
- `Cargo.toml`, `crates/noctis-image/Cargo.toml` -- add the `takumi-raster` pin (exact text in Boundaries) -- fixes the upstream drift for every fresh resolve, including bots on crates.io.
- `crates/noctis/src/lib.rs` -- `pub use twilight_model::id;` (doc: Discord ids and markers, same as `noctis::twilight::model::id`) and prelude `Id` -- the template and skill already use these names.
- `examples/project-template/Cargo.lock` -- regenerate (`cargo update --manifest-path examples/project-template/Cargo.toml -p takumi-raster --precise 0.5.8` or a plain build), then commit.
- `examples/project-template/src/**` -- fix the compile errors and clippy hits listed in the Code Map; replace `impl Ctx` helpers as described; `GuildConfig`/`StoredWebhook` ids → `u64` with `Id::new_checked` on read; remove every "planned API" NOTE and the `[planned]` comment; `cargo fmt`.
- `examples/project-template/src/modules/setup/mod.rs` -- `#[cfg(test)] mod tests`: the Kv round-trip row of the matrix with `noctis::ports::{encode_value, decode_value}` (derive `PartialEq` on both types if needed), plus the "Stored id 0" row on whatever helper turns a `StoredWebhook` into `Id`s (returns `None`) -- proves every stored setup type decodes, which `TestApp` cannot read directly.
- `examples/project-template/tests/flows.rs` -- `setup_rejects_other_users` asserts `matches!(err, TestError::NotVisible { .. })`; new `profile_edit_rejects_other_users` (owner guard on the public `/profile` reply); the existing webhook test now passes unchanged (it proves `StoredWebhook` is read back); drop the NOTE header.
- `examples/project-template/tests/snapshots/` -- generate `flows__setup_step1.snap` (`INSTA_UPDATE=always`) and `profile_card.png` (first non-CI run), inspect both (no ids/tokens that vary, card looks right via the Read tool), commit; `.gitignore` adds `*.snap.new`.
- `examples/project-template/tests/{adapters,memory,templates}.rs` -- drop NOTE lines (incl. `templates.rs:2`).
- `examples/project-template/README.md` -- replace the Status block (builds and passes its tests against this repo's `noctis`; CI gate `template-build`); add a "Size (SM-1)" section: table of code lines for `src/main.rs`, `src/modules/setup/{mod,view,handlers}.rs`, `src/modules/profile/view.rs`, their sum, the whole `src/` total, the goal ≤ 150, and the command.
- `examples/project-template/AGENTS.md` -- l.30 gives the exact gate commands (fmt, clippy default and `--all-features`, `CI=true cargo test`); new convention line: ids in `Kv` values are `u64`; mention `Id` comes from the prelude and markers from `noctis::id::marker`.
- `examples/project-template/.claude/skills/noctis/` -- `references/adapters.md` §3: note that `Id<_>` does not round-trip through `Kv` (store `id.get()`, rebuild with `Id::new_checked`), fix l.340-343 to name `noctis::id` / `noctis::twilight::model`; mention `noctis::id` and prelude `Id` wherever skill-check requires; `SKILL.md` API hash.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; paste the printed hash into `SKILL.md`.
- `.github/workflows-disabled/ci.yml` -- `template-build`: remove `continue-on-error` and the tracking comment; steps = the six template gate commands (run in `examples/project-template`, `--locked`; GitHub sets `CI=true`), keeping the libopus comment on the `--all-features` steps; update the build-config comment (l.30-31) to drop "tracking-only".
- `tools/story-loop/gates.sh` -- after `run test`, add `run template-fmt`, `template-build`, `template-clippy`, `template-clippy-all`, `template-test`, `template-test-all` with the exact commands (`CI=true` via `env CI=true cargo …`).
- `AGENTS.md` (root) -- add the template gate commands to the "Before calling Rust work done" list.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- `(decision)` lines for any implementation-time choice not already logged by planning.

**Acceptance Criteria:**
- Given the repo, when the six template gate commands run, then all pass, and the template covers `/dashboard`, `/setup start`, the tabbed paginated dashboard, `/profile` (Template in a Media Gallery), `BoundedCacheStore` with its conformance test (`tests/adapters.rs`) and `en-US`/`tr` locales.
- Given `.github/workflows-disabled/ci.yml`, when read, then `template-build` has no `continue-on-error`, runs fmt/build/clippy/test (default and all features, `--locked`), and so gates the pixel snapshot and the adapter conformance test; `tools/story-loop/gates.sh` runs the same commands.
- Given `grep -rniE "planned (noctis )?api|\[planned\]" examples/project-template/{src,tests,README.md}`, when run, then it prints nothing.
- Given the template, when `cargo run --release -p xtask -- skill-check` runs, then it passes (doctrine lint: no embeds/`content` output, no manual response type, no collectors/listeners), as do `api-snapshot`, `deps-check` and the root gates.
- Given template `README.md`, when read, then its SM-1 section reports the counted lines against ≤ 150 with the reproducible command.

## Spec Change Log

## Review Triage Log

### 2026-10-25 — Review pass
- verdicts: 22 findings — high 0, medium 4, low 9, false 9, maybe-false 0
- findings:
  - `[false]` `[reject]` (blind) `setup_flow_validates_then_finishes` component check is vacuous (top-level only) — testkit `nodes()` (crates/noctis-testkit/src/response.rs:673) walks the whole tree, so `components()` includes action rows nested in Containers; no type-1 node means no button or select remains.
  - `[false]` `[reject]` (blind) a malformed stored `StoredWebhook` makes `post_log` fail forever — the matrix row "Undecodable stored entry" settles this (warn, log not posted, setup succeeds), and no reachable path writes undecodable bytes: the template never compiled before this story, so no `Id`-typed record exists.
  - `[low]` `[defer]` (blind) old webhook not deleted on log-channel change (leak) — pre-existing: the baseline `post_log` had the same overwrite-without-delete logic; deferred to frontmatter.
  - `[low]` `[patch]` (blind) `GuildConfig` has no checked way back to typed ids, though template `AGENTS.md` points at it as the `Id::new_checked` example — patch: add `GuildConfig::ids()` mirroring `StoredWebhook::ids()` plus a unit test.
  - `[medium]` `[patch]` (blind) profile card pronouns lost their localization (`p.label().to_owned()`), `tr` users see English and `profile.pronouns.*` keys are dead — patch: map `Pronouns` to `s.profile.pronouns.*` in `load` and assert a `tr` label.
  - `[low]` `[reject]` (blind) six template gates add ~3.5 GB target and time to every gate pass, also in light mode — Design Notes D6 accepted this with `prune-target.sh` as the mitigation; a shared target dir is not a direct correction and its saving is unproven.
  - `[false]` `[reject]` (blind) exact `takumi-raster =0.5.8` pin on a published crate / unused optional dep — deliberate (D1, memlog); it prevents the takumi-core split for every fresh resolve; deps-check passes with the optional render-only shape.
  - `[low]` `[defer]` (blind) no instruction to refresh the template `Cargo.lock` under `--locked` gates — the fix edits agent-context files (AGENTS.md / gate docs); deferred to frontmatter.
  - `[low]` `[patch]` (blind) `NotVisible { user: 201 }` hardcodes MEMBER's id — patch: compare against `MEMBER.get()`. The `app.stop()` inconsistency part names no harm.
  - `[false]` `[reject]` (blind) SKILL.md still has the "planned API" v0 banner — the spec's Never list and epics Story 14.5 AC assign removal of `[planned]` markers and the `v0` status to Story 14.5.
  - `[low]` `[defer]` (blind) adapters.md Kv snippet uses elided names and lacks a migration sentence for already-stored `Id` values — elided names are the skill's fragment convention (nothing fails); the migration sentence edits the Usage Skill (agent context); deferred to frontmatter.
  - `[low]` `[reject]` (blind) SM-1 README counts are hard-coded and can go stale — the AC asks for a reported count with a reproducible command, which the README gives; the counts match the command today (no block comments in the files); a checking gate adds complexity for a report.
  - `[false]` `[reject]` (edge) undecodable `StoredWebhook` errors on every Finish — same refutation as the blind malformed-webhook row (matrix-settled, unreachable).
  - `[low]` `[defer]` (edge) old webhook orphaned on channel change — grouped with the blind leak row (pre-existing); deferred.
  - `[medium]` `[patch]` (vgap) `GuildConfig` ids are untyped `u64`, no test checks which id lands in which mention — patch: assert `<#300>` and `<@&400>` in the done text and in the webhook body.
  - `[medium]` `[patch]` (vgap) no test exercises the channel-change branch of the webhook reuse check — patch: add `setup_finish_new_log_channel_creates_a_new_webhook` with a second text channel.
  - `[false]` `[reject]` (intent 1) "planned API" notes remain in the bundled skill — Story 14.5 owns the skill's `[planned]`/v0 removal (spec Never list, epics 14.5 AC); `bounded_cache.rs` NOTE is an accurate status line, not a planned-API note.
  - `[false]` `[reject]` (intent 2) required gate edited in `.github/workflows-disabled/` — maintainer policy pauses CI until Epic 14's launch stories; live enforcement is `gates.sh`, as the spec requires.
  - `[medium]` `[patch]` (intent 3) `tr` locale not exercised at runtime and card pronouns no longer localized — grouped with the blind pronoun row; the patch adds the `tr` assertion.
  - `[false]` `[reject]` (intent 4) SM-1 file set omits `lib.rs` `configure` and profile `load` — D7 (memlog) fixed the file set deliberately and the README names it; adding files only enlarges an already reported miss.
  - `[low]` `[defer]` (intent 5) library changes in a template story; `--locked` makes every graph change need a template lock update — the library changes are spec-required (D1, D3, policy followed); the lock-refresh part is grouped with the blind lock row; deferred.
  - `[false]` `[reject]` (intent 6) the done-state check is weaker than "no action row" — `components()` is recursive (testkit `nodes()`), so the check is not vacuous.

## Design Notes

Decisions (logged in the memlog at planning):
- D1 Pin `takumi-raster =0.5.8` in `noctis-image` (not only the template lock): a bot that depends on `noctis = "0.1"` resolves fresh and would hit the same `takumi-core` split.
- D2 No `Ctx` trait: three helpers do not justify new framework API before 0.1; the template passes the accessors it needs. Story 14.5 settles the skill's `[planned] Ctx` line (deferred).
- D3 Add `noctis::id` + prelude `Id`: one-line re-exports the skill (`SKILL.md:85`, `routes-payloads-refs.md`) already documents; Payload structs need `Id` constantly.
- D4 Kv values store ids as `u64`: twilight's `Id` deserializes with `deserialize_any`, which postcard rejects. Framework-level fix deferred.
- D5 `setup_rejects_other_users` expected Discord-impossible behaviour (a click on someone else's ephemeral message); it now asserts `NotVisible`, and the owner guard is proven on the public `/profile` reply.
- D6 Gates are `--locked` with fmt, default and all-features runs (voice + prefix), mirroring CI; disk: if the lane runs low, `tools/story-loop/prune-target.sh examples/project-template/target 120`.
- D7 SM-1 is reported, not enforced. Planning measured about 460 code lines for the SM-1 files (the setup flow includes webhook logging and per-step validation beyond SM-1's minimum); the README states the miss.

SM-1 count command (code lines: non-blank, not starting with `//`, stopping at `#[cfg(test)]`):

```sh
cd examples/project-template && for f in src/main.rs src/modules/setup/{mod,view,handlers}.rs src/modules/profile/view.rs; do
  awk '/^[[:space:]]*#\[cfg\(test\)\]/{exit} {t=$0; sub(/^[[:space:]]+/,"",t); if (t=="" || t ~ /^\/\//) next; n++} END{print n+0, FILENAME}' "$f"; done
```

## Verification

**Commands:**
- `cargo fmt --all` and `cargo fmt --manifest-path examples/project-template/Cargo.toml` -- no diff afterwards with `--check`
- `cargo build --release --locked --manifest-path examples/project-template/Cargo.toml` -- ok
- `cargo clippy --release --locked --manifest-path examples/project-template/Cargo.toml --all-targets -- -D warnings` and the same with `--all-features` -- clean
- `CI=true cargo test --release --locked --manifest-path examples/project-template/Cargo.toml` and the same with `--all-features` -- all pass (flows, adapters, memory, payload_fields, templates, voice)
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- deps-check`, `-- api-snapshot`, `-- skill-check`, `-- rules-catalog`, `-- tw-grammar --check` -- ok
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- ok
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- ok
- `tools/story-loop/gates.sh` -- ALL-OK (includes the new template steps)

## Auto Run Result

- **Summary:** The Project Template (`examples/project-template/`) now builds, passes clippy and tests under `--locked` (default and all-features), and runs as gates in `tools/story-loop/gates.sh`. Library support: `takumi-raster =0.5.8` pin in `noctis-image` (D1), `noctis::id` module plus prelude `Id` re-export (D3, public-api re-blessed). Template ids stored in Kv as `u64` (D4); setup-flow test asserts `NotVisible` for other users (D5); SM-1 line count reported in the README, not enforced (D7).
- **Files changed:** `crates/noctis-image/Cargo.toml` (takumi-raster pin); `crates/noctis/src/lib.rs` (`id` module, prelude `Id`); `xtask/public-api.txt` (re-bless); `crates/noctis/tests/menus.rs`, `crates/noctis-testkit/tests/observability.rs` (adjust to API); `examples/project-template/src/**` (compile fixes, typed/u64 ids, setup webhook logging, `GuildConfig::ids()`, localized pronouns); `examples/project-template/tests/**` (flows, memory, adapters, templates, new snapshots); template `README.md`, `AGENTS.md`, `.gitignore`, `Cargo.lock`; Usage Skill `SKILL.md` and `references/adapters.md`; `tools/story-loop/gates.sh` (six template gate steps); memlog decisions.
- **Review findings (22: high 0, medium 4, low 9, false 9):**
  - Patched (6; medium 4, low 2 at entry verdict... grouped into 5 patches): localize profile pronouns via `s.profile.pronouns.*` with a `tr` assertion (medium x2, grouped); assert `<#300>`/`<@&400>` in done text and webhook body (medium); new test `setup_finish_new_log_channel_creates_a_new_webhook` (medium); `GuildConfig::ids()` checked conversion plus unit test (low); `NotVisible` compares against `MEMBER.get()` (low).
  - Deferred (5 rows): old webhook not deleted on log-channel change (pre-existing, blind+edge); no instruction to refresh template `Cargo.lock` under `--locked` (blind + intent 5); adapters.md migration sentence for already-stored `Id` values.
  - Rejected (11): vacuous component check (testkit `nodes()` is recursive; blind + intent 6); undecodable `StoredWebhook` (matrix-settled, unreachable; blind + edge); gate disk/time cost (accepted by D6, prune-target mitigation); exact takumi pin (deliberate D1); SKILL.md v0 banner and planned-API notes (owned by Story 14.5; blind + intent 1); hard-coded SM-1 counts (reproducible command given); gate edited in workflows-disabled (CI paused by maintainer); SM-1 file set (fixed by D7).
- **Follow-up review recommended: true** (first pass, 4 medium patched). Named risk: the pronoun-localization patch and the new webhook channel-change test touch setup/profile handlers after the reviewed diff; their interaction with Kv-stored `u64` ids and the webhook reuse check has not been independently re-reviewed.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, workspace clippy/test, xtask deps-check, api-snapshot, skill-check, rules-catalog, tw-grammar, tw_crosscheck, cargo deny, MSRV 1.91, plus the new template build/clippy/test steps, default and all-features). CI is paused by the maintainer (workflows in `.github/workflows-disabled/`).
- **Residual risks:** orphaned webhooks on log-channel change; `--locked` template gates fail whenever the library graph changes until the template lock is refreshed; twilight `Id` is not postcard-serializable in Kv (framework fix deferred); SM-1 line target missed (reported); Usage Skill `[planned]` markers remain until Story 14.5.
