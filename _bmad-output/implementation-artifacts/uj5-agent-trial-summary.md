# UJ-5 agent trial summary (Story 14.5, SM-3)

**Date:** 2026-10-25. **Rounds:** 1 (the bar was met, so no further round ran).
**Agents:** five headless Claude Code 2.1.310 sessions (`claude -p`, model
`claude-sonnet-5-5`), one per isolated copy of `examples/project-template/` under
`/home/claude/lanes/d/uj5-trials/round-1/trial-<n>/`. Each copy was made without `target/`,
and its `noctis` dependency pointed at `/home/claude/lanes/d/noctis-rs/crates/noctis`. Each
agent got the verbatim prompt from the spec's Design Notes. The copies were deleted
afterwards.

**Verification (implementer):** for each copy, `touch` the leaderboard sources, `src/lib.rs`,
`src/strings.rs` and `locales/*.yaml`, then run `cargo build --release --locked` and
`cargo clippy --release --locked --all-targets -- -D warnings` with the spec's
`CARGO_TARGET_DIR`. Each run printed `Compiling noctis-bot-template (<copy>)`, so the copy's
own sources were compiled. After that:
- grep `derive(Template`, `derive(SlashCommand` and `fn render_` under
  `src/modules/leaderboard/`, and `leaderboard` in `src/lib.rs`;
- scan each stream-json transcript for any `Read`/`Glob`/`Grep`/`Bash` call on a forbidden path
  (Noctis source, `~/.cargo/registry`, `~/.cargo/git`, a `target/` directory). None was found
  in any trial.

## Round 1: 4 of 5 compile (verdict: pass, SM-3 ≥ 4/5)

| Trial | Files written | Files read (all inside the copy) | First attempt (agent) | Attempts | Final build (verified) | Clippy (verified) | First compile error | Skill gaps the agent named |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `src/modules/leaderboard/{mod,view,handlers}.rs`; `src/modules/mod.rs`, `src/lib.rs`, `locales/en-US.yaml`, `locales/tr.yaml` | `AGENTS.md`; `src/{lib,strings}.rs`; `src/modules/mod.rs`; `src/modules/profile/*`; both locale files; `noctis-theme.css`; skill `SKILL.md`, `adapters.md` §3, `commands.md` §1-3, `views-and-recipes.md` §3.2, `i18n.md` §4, `images.md` §2-4; `rust-best-practices/SKILL.md` | success | 1 | **compiles** | fails: `doc_markdown` (`CacheStore` without backticks in a `//!` comment) | — | no way to list `Kv` keys (built a capped roster); `incr`'s `by` type; `color_rgb`'s argument type |
| 2 | same layout as 1 | `AGENTS.md`; `src/{lib,strings}.rs`; `src/modules/mod.rs`; `src/modules/profile/*`; both locale files; `noctis-theme.css`; `dashboard/{view,handlers}.rs` (grep); skill `SKILL.md`, `views-and-recipes.md` 1-400, `adapters.md` §3, `commands.md`/`i18n.md`/`images.md` (grep) | success | 1 | **compiles** | clean | — | no way to list `Kv` keys (capped index); how to produce the `noctis_route_width` doc line |
| 3 | same layout as 1 | `AGENTS.md` (from context); `src/{lib,strings}.rs`; `src/modules/mod.rs`; `src/modules/profile/*`; both locale files; `noctis-theme.css`; skill `views-and-recipes.md`, `adapters.md` §3, `commands.md` §1-3, `images.md` §2-3 | success | 1 | **compiles** | clean | — | no way to list `Kv` keys; `noctis_route_width` lines; `view!` helper functions returning `Node` are only hinted |
| 4 | same layout as 1 | `AGENTS.md`; `src/{lib,strings}.rs`; `src/modules/mod.rs`; `src/modules/profile/*`; both locale files; `noctis-theme.css`; `tests/templates.rs`; skill `SKILL.md`, `views-and-recipes.md` 1-400, `adapters.md` §3, `commands.md` §3, `images.md` §3 | success (stale, see below) | 1 | **fails** | fails (same error) | ``locale file `locales/en-US.yaml` has errors (see `i18n!`): simple key expect ':' at line 152 column 1`` (`points_added:"…"` without a space) | no way to list `Kv` keys; whether `cmdword.*` is needed for new words; whether to copy `rate_limit` |
| 5 | same layout as 1 | `AGENTS.md`; `src/{lib,strings}.rs`; `src/modules/mod.rs`; `src/modules/profile/*`; both locale files (`tr.yaml` partly); skill `SKILL.md`, `adapters.md`/`images.md` (grep), `commands.md` §3, `views-and-recipes.md` §3.2; `rust-best-practices/SKILL.md` | success | 1 | **compiles** | clean | — | no way to list `Kv` keys; whether `#[option(min_value, max_value)]` works without a description argument; `noctis_route_width` lines |

Every trial has a leaderboard Template, a pure `render_leaderboard` View (Media Gallery,
Refresh, Previous/Next), `/leaderboard`, and `/points add user amount` with
`permissions = MANAGE_GUILD`, all mounted in `src/lib.rs`. All five chose the same storage
design: `ctx.kv().incr` counters plus a capped per-guild index of user ids.

**First-attempt compile (the UJ-5 wording):** every agent built exactly once, so the code
the implementer verified is each agent's first attempt: **4 of 5 compile on the first
attempt.**

**Measurement caveat:** all five copies share one `CARGO_TARGET_DIR` and one package name
(`noctis-bot-template`), and they built at the same time. Trial 4's own build and trial 1's
own clippy run reported success even though the code fails. Cargo had marked the shared
fingerprints fresh, so those agents saw a stale result and stopped. The implementer's
forced rebuilds are the numbers recorded above. A future trial should give each copy its
own package name, or build the copies one at a time.

## Skill changes made after the round

The round met the bar, so no further round ran. These gaps were still added to the skill:

- `references/adapters.md` §3: `Kv` cannot list or scan keys. To rank entries, keep an
  id index next to the `incr` counters, or use your own database Adapter. The
  `incr` row now states its types (`by: u64`, `ttl: Option<Duration>`). Migrating a stored
  `Id<_>` now also mentions `Kv::schema(n)`.
- `references/patterns.md` `## Common mistakes` gained three rows: no `Kv` key listing; the
  `i18n!` YAML error `simple key expect ':'` (no space after the colon); clippy's
  `doc_markdown` (identifiers in doc comments need backticks).

### Gaps not changed

- `noctis_route_width` doc line: already documented in `references/routes-payloads-refs.md`
  (the generated `__noctis_route_width_<StructName>` test and its command) and in
  `AGENTS.md`.
- `color_rgb` argument type: already documented in `references/images.md` §4
  (`0xRRGGBB`, the low 24 bits are used).
- `view!` helper functions returning `Node`: `references/images.md` shows them
  (`fn(&T) -> Node` components) only briefly; remains open.
- `#[option(min_value, max_value)]` without `description`: `references/commands.md`
  shows the arguments only together with `description`; whether it is optional remains
  open.
- `cmdword.*` for new words: already documented in `references/i18n.md` and
  `references/concept-map.md`.
- Copying `rate_limit`: already covered by `AGENTS.md` and `references/patterns.md`
  (anti-patterns).
