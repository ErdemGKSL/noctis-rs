---
title: "Reconciliation: brief + addendum vs PRD"
created: 2026-10-19
inputs:
  - ../../briefs/brief-noctis-rs-2026-10-19/brief.md
  - ../../briefs/brief-noctis-rs-2026-10-19/addendum.md
target: prd.md
---

# Reconciliation: brief + addendum vs PRD

Scope: capabilities, qualitative principles and scope decisions in the brief and its
addendum that the PRD misses or contradicts. Pure implementation detail is excluded.
Suggested FR text uses placeholder ids (`FR-B1` …); renumber when merging into the PRD.

Severity: **High** = contradiction or missing core principle; **Med** = missing
capability or testable consequence; **Low** = wording or traceability.

## Summary

| # | Gap | Type | Severity |
|---|-----|------|----------|
| B1 | UI doctrine (two surfaces, images read-only, CV2 for anything interactive) is not stated anywhere normative | Missing principle | High |
| B2 | "Slash commands are bootstrap entry points" is only implicit in UJ-1 | Missing principle | High |
| B3 | Automatic pagination on overflow vs PRD's explicit Paginator + hard error | Contradiction | High |
| B4 | Adapter principle "everywhere" reduced to a closed list; Module flag state and Discord cache are not Adapters | Partial contradiction | High |
| B5 | "Rules for writing high-quality interfaces" missing from Usage Skill scope | Missing capability | Med |
| B6 | "Steps over pagination for setup flows" / "Navbar when it doesn't fit one container" choice guidance missing | Missing principle | Med |
| B7 | Allocation-conscious design / no per-event allocations has no NFR | Missing NFR | Med |
| B8 | Prefix commands: brief says out unless research shows dependence; PRD puts FR-14 in MVP | Contradiction | Med |
| B9 | "All examples and docs use id-routed handlers" not enforced on docs/template | Missing consequence | Med |
| B10 | Command responding directly with a View (no hand-written response plumbing) not an explicit FR | Missing capability | Med |
| B11 | Secondary audience (serenity/poise migrants) and tescord migration doc absent | Traceability | Low |
| B12 | Vision items (UI pattern library, image template library) not captured as post-v1 direction | Traceability | Low |

---

## B1. UI doctrine is not normative (High)

**Source:** addendum "UI doctrine" bullets 1–2; brief "Components V2 as the main UI".
**PRD state:** §1 Vision and §4.4/4.5 descriptions mention CV2 and images, but no
principle says *which surface to use when*. Nothing prevents (or warns against)
interactive flows built on embeds/plain content, or images used for detailed data
a user must act on.

**Suggested addition** — a new §4.0 "Product Principles" (referenced by FR-49/FR-50),
plus one FR:

> **Principle P-1 (Two surfaces).** Noctis has two primary output surfaces:
> Components V2 messages for everything interactive or detailed, and generated
> images for clean, dynamic, read-only visuals. Plain `content` and embeds are
> supported only as secondary/legacy output and are never used by built-in widgets,
> the Project Template or the Usage Skill examples.

> **FR-B1: Surface doctrine in docs and template.** The Usage Skill and the Project
> Template present Components V2 as the default response for every Command and
> Component Handler, and images only as embedded read-only media (Thumbnail, Media
> Gallery).
> **Consequences (testable):**
> - No Project Template handler responds with an embed or with a plain-`content`
>   message as its primary output.
> - The Usage Skill contains a "which surface?" decision section (interactive →
>   CV2; dense read-only visual → Template image inside a CV2 message).

## B2. "Slash commands are bootstrap entry points" (High)

**Source:** addendum "Slash commands are mostly bootstrap entry points"; brief
Executive Summary (`/setup start` takes no parameters).
**PRD state:** shown only in UJ-1. FR-7/FR-10 put weight on option structs; nothing
states the preferred pattern or makes the zero-option → View path first-class.

**Suggested addition:**

> **Principle P-2 (Commands open apps).** A chat-input Command is primarily an entry
> point that opens a View. Configuration and data entry happen inside the View
> (selects, modals, Wizard Steps), not through long option lists.

> **FR-B2: Option-less entry Commands.** A bot author can register a Command whose
> handler returns a View, and Noctis sends it as the initial response (applying
> FR-21 deferral and the V2 flag) without further response code.
> **Consequences (testable):**
> - A Command declared with no options and a View-returning handler compiles and
>   publishes with an empty option list.
> - The Usage Skill recommends this pattern and lists when options are still
>   appropriate (quick actions, autocomplete lookups, context-menu targets).

(B10 below is the capability half of this principle; merge them if preferred.)

## B3. Automatic pagination vs hard error (High, contradiction)

**Source:** brief "automatic pagination when content exceeds component limits";
addendum "When a view exceeds the component limit, it paginates."
**PRD state:** FR-24 returns an error on Limit violations (lenient mode only
truncates text). FR-28 Paginator paginates only when the author explicitly uses it.
So a View that grows past the Limits fails rather than paginating, which contradicts
the brief.

**Suggested resolution (pick one and state it):**

> **FR-B3: Overflow policy for Views.** A View declares an overflow policy for its
> repeatable content: `Paginate` (default for lists), `Error`, or `Truncate`.
> With `Paginate`, content that would exceed any Limit is split into pages by the
> Paginator automatically, keeping fixed parts (header, Navbar) on every page.
> **Consequences (testable):**
> - A View whose list grows from 10 to 300 items produces a paginated message with
>   no code change and no Limit error.
> - Content that is not marked repeatable still yields the FR-24 error naming the
>   path and Limit.

If the PRD intentionally keeps pagination explicit, record the deviation from the
brief in §7 Non-Goals or §10 Open Questions so it is a decision, not an omission.

## B4. Adapter principle "everywhere" narrowed to a closed list (High)

**Source:** addendum "Adapter principle (applies project-wide)": "not limited to
the reference store … apply it everywhere it makes sense"; brief "Pluggable adapters
everywhere".
**PRD state:** FR-38 lists seven Adapters as a closed set. Two storage-like concerns
introduced elsewhere in the PRD are not Adapters:
- **Module enable/disable state** (FR-45, global and per guild). In a Cluster
  (UJ-3) this state must be shared across processes, but no Adapter holds it.
- **Discord resource cache** (FR-40) is configurable but not pluggable, which
  conflicts with "everywhere" and with the brief's out-of-scope line ("sharded-cache
  clustering *beyond the adapter traits*", implying an adapter trait exists).
- Publish state / last-published hash (FR-13) and Wizard/Navbar state are fine as
  Refs, but this should be stated.

**Suggested FR text:**

> **NFR/Principle P-3 (Adapter rule).** Every framework feature that stores, caches
> or shares data across Shards or processes does so through an Adapter trait with an
> in-memory default. A new feature that introduces such state must add or reuse an
> Adapter; it may not hold process-local state that breaks under a Cluster.

> **FR-B4a: FlagStore Adapter.** Module enable/disable state (FR-45) is read and
> written through a FlagStore Adapter. **Consequence:** disabling a Module for a guild
> in one process takes effect in every process of the Cluster that shares the
> FlagStore, within a documented propagation delay.

> **FR-B4b: Discord cache seam.** State explicitly whether the Discord resource cache
> (FR-40) is an Adapter in v1. If not, add it to §7 Non-Goals ("the Discord resource
> cache is process-local in v1") so the "everywhere" rule has a documented
> exception.

## B5. "Rules for writing high-quality interfaces" (Med)

**Source:** addendum "The framework documents Components V2 limits and allowed
components one by one. It also encodes the rules for writing high-quality
interfaces."
**PRD state:** FR-50 covers Limits, signatures, patterns and anti-patterns, but not
a per-component reference ("one by one") nor UI-quality rules.

> **FR-B5: UI component reference and quality rules.** The Usage Skill contains
> (a) one entry per Components V2 component and modal component, with where it may
> appear, its Limits and its Noctis builder, and (b) a UI quality rulebook: one
> primary action per View, the active state is visible (disabled tab), destructive
> actions confirm, ephemeral for single-user UI and public for shared content, every
> interactive View has a way back or a cancel, empty and error states are designed.
> **Consequence (testable):** every component type in FR-23/FR-29 has an entry; CI
> fails if a builder exists without one.

## B6. Choosing Wizard vs Navbar vs Paginator (Med)

**Source:** addendum "Setup-style flows split content logically, not by page. Prefer
steps with a progress indicator over pagination." and "General-purpose views that
don't fit one container use a navbar."
**PRD state:** the three widgets exist (FR-26–28) but nothing states when each is
the right one.

> **FR-B6: Navigation pattern guidance.** The Usage Skill and Wizard/Navbar/Paginator
> docs state the selection rule: setup/config flows → Wizard (logical Steps with
> progress, never "page 2 of 3"); multi-topic dashboards → Navbar tabs; homogeneous
> lists → Paginator, optionally inside a Navbar tab. The Project Template's examples
> follow the rule.

## B7. Allocation-conscious design (Med)

**Source:** brief "Events dispatch to handlers while avoiding per-event allocations
wherever possible"; addendum "Memory allocation stays small … allocation-conscious
algorithms throughout".
**PRD state:** NFR-1 bounds idle memory per guild; NFR-3 bounds latency. Nothing
bounds allocation on the hot path.

> **NFR-B7 Allocation.** The dispatch path (event arrival → handler start, and
> `custom_id` → Route → decoded Payload) has a documented allocation budget measured
> by a CI benchmark. `[ASSUMPTION: target set in architecture, e.g. ≤ N heap
> allocations per dispatched interaction excluding handler code]`

## B8. Prefix commands scope (Med, contradiction)

**Source:** brief Scope "Out for v1: Prefix and message commands, unless research
shows tescord users depend on them."
**PRD state:** FR-14 is in MVP (§8.1) as an assumption.
**Research evidence:** tescord dropped prefix commands; they are dbi-only and rated
P2; schwi-v2's `s!` system is "unrelated to slash emulation" (tescord-features §3.20).
The research therefore does **not** show dependence, so the brief's condition for
inclusion is not met.

> **Resolution:** move FR-14 to §8.2 Out of Scope for MVP (deferred to v1.x), or keep
> it and record in §11 that this deliberately overrides the brief, with the reason.

## B9. Docs and examples never use per-message listeners (Med)

**Source:** brief Success Criteria "All examples and docs use id-routed handlers with
the reference store."
**PRD state:** NFR-2 forbids per-message state in the framework, but docs/template
are not constrained, and no non-goal rules out collector/inline-listener APIs.

> **FR-B9 / consequence on FR-49 and FR-50:** No example, doc snippet or Usage Skill
> pattern uses a per-message collector, await-component loop or anonymous inline
> listener; all interactive state uses Routes with Payloads/Refs. Add to §7 Non-Goals:
> "No collector or inline-listener API (per-message closures)."

## B10. Command → View response plumbing (Med)

See B2. Also covers updates: FR-25 says the same View renders first response and
updates, but there is no FR saying a Component Handler can return a View and Noctis
picks the correct response type (update message vs new message vs deferred edit).

> **FR-B10: View responses.** A Command, Component Handler or Modal Handler can return
> a View (or a View plus attachments). Noctis chooses the response type: initial
> reply for Commands, in-place update for components on the same message, and the
> deferred variants under FR-21. **Consequence:** the Project Template contains no
> manual response-type selection.

## B11. Secondary audience and migration path (Low)

**Source:** brief "Secondary: Rust developers coming from serenity/poise or from
TypeScript frameworks … migrating a tescord or dbi bot feels natural."
**PRD state:** §2.1 covers tescord/dbi only; serenity/poise migrants are absent; no
migration documentation is required.

> **FR-B11: Migration guide.** Documentation includes a tescord/dbi → Noctis concept
> map (Pack→Module, `buildComponent`→typed Payload, `locale.user`→User Locale, etc.)
> and a short serenity/poise → Noctis section.

## B12. Vision items not captured (Low)

**Source:** brief Vision: adapter ecosystem crates, "a library of reusable UI
patterns and image templates", type generation for locales and payloads.
**PRD state:** Redis/SQL crates are deferred in §8.2; the UI-pattern and image-template
library is not mentioned anywhere.

> Add to §8.2: "A library of reusable Views and Templates (cards, leaderboards,
> charts) beyond the Project Template examples is deferred to v1.x." Optionally ship
> one standard card Template (the NFR-3 benchmark card) as a public example.

## Items checked and found consistent

Sharding/identify/resume/shard ranges (FR-2–4); space-separated Command Paths and
auto-publish with diff (FR-7, FR-13); one handler per id with inline data or Ref
(FR-15–17); in-memory defaults (FR-38); takumi + Tailwind, PNG/WebP/GIF, bounded pool,
optional feature (FR-32–34, FR-48); i18n for metadata and responses (FR-41–43);
feature toggles instead of hot reload (FR-45, §7); voice, web dashboard, non-tokio out
(§7); ~150-line success criterion (SM-1); schwi successor (SM-2).
