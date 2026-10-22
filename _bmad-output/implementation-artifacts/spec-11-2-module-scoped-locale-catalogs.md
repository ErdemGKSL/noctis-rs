---
title: 'Story 11.2: Module-scoped Locale catalogs'
type: 'feature'
created: '2026-10-22'
status: 'in-progress'
baseline_revision: '2818c396c94fa49fb44fa61e1bf754a289129d15'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-11-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 11.1 lets only the App register locale catalogs (`AppBuilder::locales`). A Module cannot ship its own `i18n!` catalog, so a reusable Module is not self-contained. A key clash between catalogs names two `<crate>/<dir>` origins but never the Modules that own them (FR-72, FR-75; AD-7, AD-16).

**Approach:** Add `Module::locales(catalog)`, which stores the catalog as Module data. At `build()`, core collects the App's catalogs (in call order), then every mounted Module's catalogs (in fold order). It merges them through the existing `noctis_i18n::AppStrings` engine, so the merged catalog serves every Module's keys. A clash becomes `BuildViolation::DuplicateLocaleKey`, which now also names the owner of each side: a Module id, or the App. The merged strings are App-wide and never depend on Module state. A Module's strings therefore stay available to every other Module, including after Story 13.3 adds runtime disabling.

## Boundaries & Constraints

**Always:**

- **`Module::locales`**
  - Signature: `pub fn locales(mut self, catalog: noctis_i18n::Catalog) -> Self`, marked `#[must_use = "Module::locales returns the Module with the catalog added"]`.
  - It is ungated in core, like `AppBuilder::locales`. It pushes onto a new `locales: Vec<noctis_i18n::Catalog>` field.
  - It has a `pub(crate) fn catalogs(&self) -> &[noctis_i18n::Catalog]` accessor.
  - The rustdoc example builds a `Catalog::__new` static, the same way the `AppBuilder::locales` doc does.
  - `Module`'s `Debug` gains `.field("locales", &origins)`, where `origins` is the `Vec<&str>` of `Catalog::origin()`. Insert it after `after` and before `children`.
- **Collection order.** The merge input is:
  1. the App's catalogs in `AppBuilder::locales` call order, each owned by `LocaleOwner::App`;
  2. then every mounted Module's catalogs in `Module::locales` call order, each owned by `LocaleOwner::Module(module.id())`. Modules are visited in the same pre-order as `Registry::fold`: roots in mount order (plugin-mounted trees included), each parent before its children, children in `module` order.

  Write this traversal as a private `fn locale_sources(app: &[Catalog], roots: &[Module]) -> Vec<(Catalog, LocaleOwner)>` in `crates/noctis-core/src/app.rs`. Use an explicit stack or recursion, and do not touch `Registry::fold`.
- **Merge.** Call `AppStrings::new` with the collected catalogs, unchanged. Map each `DuplicateKey { key, first, second }` to the owners of `first` and `second` by origin. The owner of an origin is the owner of the **first** source with that origin, because `AppStrings` dedups by origin and keeps the first.
  - A Module with catalogs but no handlers still contributes them.
  - A Module's `noctis.*` overrides apply App-wide.
- **`LocaleOwner`** is new in `crates/noctis-core/src/error.rs`. Re-export it from the `noctis-core` root and from the facade (`crates/noctis/src/lib.rs`, next to `BuildViolation`):

  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  #[non_exhaustive]
  pub enum LocaleOwner { App, Module(&'static str) }
  ```

  Display: `App` prints ``AppBuilder::locales``, and `Module(id)` prints ``Module `<id>` ``. Every variant and field has a doc comment.
- **`BuildViolation::DuplicateLocaleKey`** gains `first_owner: LocaleOwner` and `second_owner: LocaleOwner`, documented. The existing `key`, `first` and `second` fields stay. The message becomes ``locale key `{key}` is defined by two catalogs: `{first}` ({first_owner}) and `{second}` ({second_owner})``, for example ``locale key `shop.title` is defined by two catalogs: `bot/mod_shop` (Module `shop`) and `bot/mod_clash` (Module `admin`)``.
  - Violations keep `AppStrings`' order: registration order, then key order. They stay in the same place among `build()`'s checks.
- **Docs.** Update the `AppBuilder::locales` and `build()` rustdoc and the `Module` type docs. Say that catalogs from `Module::locales` merge with the App's, and that strings are App-wide and are never removed when a Module is disabled.
- Template and skill reconciliation, the API snapshot re-bless and the `API hash` line all ship in the same change (AGENTS.md).

**Never:**

- Do not change `noctis-i18n`'s public API. `AppStrings::new`, `DuplicateKey` and `Catalog` stay as they are.
- Do not filter strings by any Module state, per-guild or global, and add no Module-scoped lookup. A key resolves App-wide.
- No Boot Flag filtering of catalogs (Story 13.2), no Module id uniqueness check (Story 13.1), no `#[command(.., locales = ..)]` or command-key validation (Story 11.4), no `LocaleSource` (Story 11.5), and no `ctx.*_locale()` type change (Story 11.3).
- No equal-value exemption: two catalogs that both define a key are a violation even when the texts are identical. That includes `cmdword.*` and `noctis.*` keys.
- No debug builds, no new trybuild `pass` suites, and no weakening of `.cargo/config.toml`.

## I/O & Edge-Case Matrix

`S` = catalog `bot/mod_shop` (`shop.title`: en-US "Shop" / tr "Dükkan"). `A` = catalog `bot/mod_admin` (`admin.done`). `C` = catalog `bot/mod_clash` (`shop.title`). `P` = the App-level catalog `bot/app` (`common.ok`).

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Two Modules, disjoint keys | `Module::new("shop").locales(S)`, `Module::new("admin").locales(A)` | builds; `strings.translate(Tr, "shop.title") == "Dükkan"` and `admin.done` resolves | none |
| 2 | App and Module catalogs together | `.locales(P)` and Module `shop` with `S` | both keys resolve | none |
| 3 | Module-vs-Module clash | `shop` has `S`, `admin` has `C` | `DuplicateLocaleKey { key: "shop.title", first: "bot/mod_shop", second: "bot/mod_clash", first_owner: Module("shop"), second_owner: Module("admin") }`, with the exact message from Boundaries | `build()` `Err`, collected with the other violations |
| 4 | App-vs-Module clash, Module mounted first | `.module(shop with C)` then `.locales(S)` | `first_owner: App` (App catalogs come first whatever the call order), `second_owner: Module("shop")`; message `` `bot/mod_shop` (AppBuilder::locales) and `bot/mod_clash` (Module `shop`) `` | `Err` |
| 5 | Three-way clash | `shop` S, `admin` C, `extra` a third catalog with `shop.title` | two violations: (shop, admin), then (shop, extra) | `Err` |
| 6 | Several clashing keys | two catalogs share `a.x` and `a.y` | two violations in key order (`a.x`, `a.y`) | `Err` |
| 7 | Same catalog in two Modules | `shop` S, `admin` S | deduplicated; no violation; owner is `shop` | none |
| 8 | Same catalog in App and Module | `.locales(S)` and `shop` with S | deduplicated; no violation | none |
| 9 | One catalog twice in one Module | `Module::new("shop").locales(S).locales(S)` | deduplicated | none |
| 10 | Two catalogs in one Module that clash | `Module::new("shop").locales(S).locales(C)` | violation with both owners `Module("shop")` | `Err` |
| 11 | Nested child Module | `Module::new("ops").module(Module::new("ops/audit").locales(A))` | `admin.done` resolves | none |
| 12 | Child clashes with parent | parent `ops` has S, child `ops/audit` has C | `first_owner: Module("ops")`, `second_owner: Module("ops/audit")` (pre-order) | `Err` |
| 13 | Sibling order is pre-order, not breadth-first | roots `[r1{child c1 with C}, r2 with S]` | first is `c1` (fold order r1, c1, r2) | `Err` |
| 14 | Strings-only Module | `Module::new("strings").locales(S)` with no handlers | `shop.title` resolves | none |
| 15 | Cross-Module reference (AC 3) | `admin`'s command does `ctx.reply_text(shop_keys::SHOP_TITLE)`; `shop` owns S | a `tr` user gets "Dükkan"; `en-US` gets "Shop" | none |
| 16 | Module catalog with another default locale | a catalog with default `tr` and only `tr.yaml` | `translate(De, key)` gives the `tr` text (that catalog's own chain) | none |
| 17 | Module `noctis.*` override | a Module catalog with `noctis.expired` in `tr` | App-wide `tr` notice replaced; another Module's expired notice also uses it | none |
| 18 | Two Modules override one `noctis.*` key | both define `noctis.expired` | `DuplicateLocaleKey` naming both Modules | `Err` |
| 19 | Empty catalog | a catalog with no keys in a Module | no effect, no violation | none |
| 20 | No catalogs anywhere | Modules without `.locales` | strings behave like `Builtin` (11.1 tests unchanged) | none |
| 21 | Unicode or path-like Module id | `Module::new("çay/ölç")` clashes | message contains ``Module `çay/ölç` `` byte for byte | `Err` |
| 22 | Many Modules | 64 Modules, each with its own distinct one-key catalog | all 64 keys resolve; no violation | none |
| 23 | Debug | `format!("{:?}", Module::new("m").locales(S))` | contains `locales: ["bot/mod_shop"]` | none |
| 24 | `LocaleOwner` Display | `App` / `Module("x")` | `AppBuilder::locales` / ``Module `x` `` | none |
| 25 | Clash plus other violations | row 3 together with a duplicate `.state` | both reported in one `BuildError`, in the documented order | `Err` |

</intent-contract>

## Code Map

- `crates/noctis-core/src/module.rs:137-150` (`Module` fields), `:167` `new`, `:250` `module` (doc-example pattern), `:356-409` (`pub(crate)` accessors), `:411-443` (`Debug`): add the `locales` field, the method, the accessor and the Debug field.
- `crates/noctis-core/src/app.rs`:
  - `:1301` `catalogs` field and `:1386` its init;
  - `:1530-1558` `AppBuilder::locales` docs;
  - `:2040-2062` `build()` docs (error order);
  - `:2145-2160`, where `Registry::fold(&self.modules, ..)` runs and `AppStrings::new(&self.catalogs)` maps duplicates. Replace the map with `locale_sources` plus owner mapping.
  - `:2269-2300` builder `Debug` (unchanged).
- `crates/noctis-core/src/registry.rs:140-187` `fold`: read-only reference for the pre-order. It pops a stack seeded with `roots.rev()` and pushes `children().rev()`.
- `crates/noctis-core/src/error.rs:162-171` `DuplicateLocaleKey`: add the owner fields, the new message and the new `LocaleOwner` enum.
- `crates/noctis-core/src/lib.rs`: re-export `LocaleOwner` where `BuildViolation` is re-exported.
- `crates/noctis-core/src/app/tests.rs:247-305`: `SHOP` static and the `duplicate_locale_key` / `locales_are_idempotent` tests. Update the struct literal and message to the new fields, and add rows 1-14 and 16-25 here with hand-written `Catalog::__new` statics.
- `crates/noctis-i18n/src/catalog.rs`: `Catalog`, `AppStrings::new` (origin dedup keeps the first; violations follow registration order, then key order), `DuplicateKey`. Reuse it unchanged.
- `crates/noctis/src/lib.rs:573` (facade `pub use noctis_core::{.., BuildViolation, ..}`): add `LocaleOwner`. The `i18n` feature docs (~:560) mention `AppBuilder::locales`; add `Module::locales`.
- `crates/noctis/tests/i18n_app.rs`: pattern for a `TestApp` built with `i18n!` modules and `.locale("tr")` runs (`noctis-testkit` `runs.rs:44` `locale`).
- `crates/noctis/tests/fixtures/i18n/app/`: fixture layout. Fixtures need a leading `#` comment line naming the story.
- `crates/noctis-core/src/responder/ergonomics.rs:79` `From<MessageKey> for NoticeText`, so `ctx.reply_text(key)` resolves in the response locale.
- `.github/workflows-disabled/ci.yml:63-64`: the `i18n,testkit` step that runs `--test i18n_app`.
- `examples/project-template/src/strings.rs:15-17` (the "`Module::locales` … [planned, Story 11.2]" text) and `examples/project-template/AGENTS.md:49`.
- Usage Skill:
  - `examples/project-template/.claude/skills/noctis/references/i18n.md:96-104` (registering, `DuplicateLocaleKey`, "[planned, Story 11.2]") and `:181-186` (§5 Module-scoped catalogs);
  - `references/modules-middleware-ratelimits.md:23` ("Module-scoped strings [planned]") and `:34` ("Implemented:" list);
  - `SKILL.md:23` (API hash).
- `xtask/public-api.txt:323-326` (`DuplicateLocaleKey` fields): re-bless.

## Tasks & Acceptance

**Execution:**

- [ ] `crates/noctis-core/src/error.rs`, `src/lib.rs`: add `LocaleOwner` with its Display, and extend `DuplicateLocaleKey` with `first_owner`/`second_owner` and the new message. Re-export `LocaleOwner`. This is the typed error naming both Modules.
- [ ] `crates/noctis-core/src/module.rs`: add the `locales` field, `Module::locales` with its doctest, the `pub(crate) catalogs()` accessor, the Debug field, and a sentence in the type docs. This is the descriptor surface.
- [ ] `crates/noctis-core/src/app.rs`: add `locale_sources` (pre-order). `build()` merges App and Module catalogs and maps owners by first origin. Update the `AppBuilder::locales`, `module` and `build()` docs. This is the merge.
- [ ] `crates/noctis-core/src/app/tests.rs`: update the two 11.1 tests to the new fields and message, and add unit tests for Matrix rows 1-14 and 16-25. Each test is named after its scenario and carries a `/// Story 11.2 Matrix N` doc comment. Row 22 builds its 64 catalogs with `Box::leak` over `Catalog::__new` data, or with a `const` array. This covers the I/O matrix.
- [ ] `crates/noctis/tests/fixtures/i18n/{mod_shop,mod_admin,mod_clash}/en-US.yaml` (+ `mod_shop/tr.yaml`) and `crates/noctis/tests/i18n_modules.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`):
  - three `mod x_strings { noctis::i18n!(dir = "tests/fixtures/i18n/mod_x"); }` blocks;
  - a `TestApp` with `Module::new("shop").locales(shop_strings::catalog())` plus a `/shop` command, and `Module::new("admin").locales(admin_strings::catalog())` plus an `/admin` command that does `ctx.reply_text(shop_strings::keys::SHOP_TITLE)`;
  - a check that `/admin` with `.locale("tr")` answers "Dükkan" and without a locale answers "Shop" (rows 1 and 15, AC 1 and 3);
  - a second test that mounts `admin` with `mod_clash`, builds with `TestApp::build`, and asserts the `Err`'s violations equal the row 3 value and message (AC 2).

  This is the outermost-surface proof.
- [ ] `.github/workflows-disabled/ci.yml`: add `--test i18n_modules` to the line-64 command, and keep its comment accurate. The gate stays in sync.
- [ ] `crates/noctis/src/lib.rs`: add the `LocaleOwner` re-export, and mention `Module::locales` in the i18n feature docs. This is the facade.
- [ ] `examples/project-template/src/strings.rs`, `examples/project-template/AGENTS.md`: replace the planned text with the real `Module::locales` behavior. The template keeps one App-level catalog: that is the recommended shape for a single-crate bot, and `Module::locales` is for Modules shipped as their own crates or with their own `dir`. Add one AGENTS.md clause saying so. This reconciles the template.
- [ ] Usage Skill:
  - `references/i18n.md` §2: drop "[planned, Story 11.2]" and show the new `DuplicateLocaleKey` fields and message.
  - `references/i18n.md` §5: give the exact signature, the merge order, origin dedup, the owner names, App-wide resolution, `noctis.*` overrides being App-wide, strings that survive disabling, and an example with its own `i18n!` `mod` and `dir`.
  - `references/modules-middleware-ratelimits.md`: drop "[planned]" from `.locales(..)` and add it to "Implemented".
  - Run `cargo run --release -p xtask -- api-snapshot --bless`, then set `SKILL.md`'s `API hash` to the printed hash.

  This is the AC for the skill and API gates.
- [ ] `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: append the planning `(decision)` lines from Design Notes, plus any made during implementation. This keeps traceability.

**Acceptance Criteria:**

- Given two Modules each calling `.locales(<its i18n! catalog()>)`, when `App::build` (through `TestApp::build`) merges them, then a handler in either Module resolves both Modules' keys in the response locale (`tr` and `en-US`).
- Given two mounted Modules whose catalogs define one key, when `build()` runs, then it returns a `BuildError` whose `BuildViolation::DuplicateLocaleKey` names the key, both catalog origins and both Modules (`first_owner`/`second_owner`), before any connection, and a unit test asserts it.
- Given a Module whose catalog is the only owner of a key, when another Module's handler sends that `MessageKey`, then the translated text is sent, and the strings come from the App-wide `AppShared::strings`, which no Module state consults. The `Module::locales` rustdoc and the skill state that disabling a Module never removes its strings.
- Given the finished story, when the gates run, then the API snapshot matches the re-blessed `xtask/public-api.txt`, the skill's `API hash` equals it, and `skill-check` passes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **AC 3 holds by construction today.** Runtime enable/disable (FR-45) ships in Story 13.3, so no "disabled" state exists yet to test. The merged `AppStrings` is built once at `build()` from every mounted Module and is held in `AppShared::strings`. Lookup is by key only. Story 13.3 must keep it that way and add a test that a disabled Module's key still resolves from another Module. Log this as a `(decision)` for 13.3.
- **Boot Flags.** AD-7 lists locale catalogs among the descriptors that Boot Flags filter before the fold. Story 13.2 decides whether an unselected Module's catalog is dropped. Story 11.2 merges every mounted Module.
- **Why owners are mapped in core.** `noctis-i18n` (layer 3) knows nothing about Modules, and its `DuplicateKey` already carries origins. `AppStrings` keeps the first catalog for an origin, so the first source with that origin owns it. This keeps 11.1's engine unchanged, as the epic context asks ("`Module::locales` reuses this engine").
- **Why App catalogs come first whatever the call order.** `AppBuilder::module` and `AppBuilder::locales` may be called in any order, and Module trees can be mounted later by plugins. A fixed "App, then fold order" makes `first` deterministic and matches the Registry's fold.
- **Why the same catalog in two Modules is not a clash.** A clash means two *definitions*. One catalog shared by two Modules, or a tree mounted twice, defines each key once. This is 11.1's idempotent registration (its Matrix row 62).
- **Why no equal-value exemption.** The story says "a key defined by two Modules" fails, and two Modules that both translate `cmdword.set` should agree on one owner. Story 11.4 may revisit this for `cmdword.*` and log it there.
- **Golden mapping:**

  ```rust
  let sources = locale_sources(&self.catalogs, &self.modules);
  let owner = |origin| sources.iter().find(|(c, _)| c.origin() == origin).map_or(LocaleOwner::App, |(_, o)| *o);
  let catalogs: Vec<Catalog> = sources.iter().map(|(c, _)| *c).collect();
  match AppStrings::new(&catalogs) { Err(d) => /* DuplicateLocaleKey { .., first_owner: owner(d.first), second_owner: owner(d.second) } */ }
  ```

## Verification

**Commands:**

- `cargo fmt --all`: expected no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: expected clean.
- `cargo test --release --workspace --all-features`: expected all pass, including the `noctis-core` app tests, `tests/i18n_modules.rs` and the doctests.
- `cargo test --release -p noctis --features i18n,testkit --test i18n_app --test i18n_modules`: expected pass.
- `cargo hack test --release --workspace --each-feature`: expected all pass.
- `cargo run --release -p xtask -- deps-check`: expected ok.
- `cargo run --release -p xtask -- rules-catalog`: expected ok (no rule rows change).
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot`: expected a match. If they are missing, first install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 per AGENTS.md.
- `cargo run --release -p xtask -- skill-check`: expected ok.
- `cargo deny check`: expected ok.
- `cargo +1.91 check --release --workspace`: expected ok.
- `test ! -d target/debug`: expected no debug profile.
