---
title: "Reconciliation: tescord research vs PRD"
created: 2026-10-19
inputs:
  - ../../research/tescord-features.md
target: prd.md
---

# Reconciliation: tescord research vs PRD

Scope: tescord/dbi/schwi-v2 features (research §2 catalogue, §3, §5, §6) and known
bugs (§6.15) checked against the PRD. Implementation detail (ArcSwap, bincode, moka,
etc.) is excluded. Suggested FR text uses placeholder ids (`FR-T1` …); renumber on
merge. "Pn" is the research's priority.

## Summary

| # | Gap | Research ref | Prio | Severity |
|---|-----|--------------|------|----------|
| T1 | Duplicate Command Path / sub-name across Modules not an error ("first wins" for subcommands) | §3.3 publish step 3, §6.15 | P0 | High |
| T2 | Base-command and group descriptions come from "first non-empty"; no explicit declaration | §3.3 step 4 | P0 | High |
| T3 | Required-before-optional option order not enforced | §3.4 | P0 | High |
| T4 | Building a component for an unregistered Route is not prevented | §3.8, §3.9 | P0 | High |
| T5 | Route default component options + per-render overrides, modal pre-fill | §3.8, §5.4 | P0 | High |
| T6 | Payload schema change after deploy (stale buttons) has no defined behaviour | §4.3.1 | P0 | High |
| T7 | Refs not namespaced per App when Adapters are shared (FR-47) | §4.2, §4.3.2 | P1 | High |
| T8 | Autocomplete error / missing handler behaviour and separate autocomplete error sink | §3.5, §3.14 | P0 | Med |
| T9 | Rate limits: Message scope, dynamic `set_rate_limit`, customisable rate-limit response | §3.18, §6.9 | P1 | Med |
| T10 | Boot-time feature flags (`.flag("x")`, CLI `--features`) | §3.19, §5.9 | P1 | Med |
| T11 | Lifecycle events (ready, published, shard ready, module enabled/disabled) | §3.14, cat. #25 | P1 | Med |
| T12 | Locale mapping base code → Discord variants; user→guild→default fallback chain; per-Module locale files | §3.10–3.12 | P0 | Med |
| T13 | Multi-client: one Module tree on several Apps, per-Command target App, guild filter | §3.16, cat. #27 | P1 | Med |
| T14 | Payload extensibility (custom field types / codec hook) | §4.1, §4.3.4, cat. #14 | P0 | Med |
| T15 | Publish "clear" and per-App publish | §3.17 | P1 | Low |
| T16 | Fallback / prefix Route matching (Inspector equivalent) | §3.15 | P2 | Low |
| T17 | Option validators | §3.4, cat. #33 | P2 | Low |
| T18 | Inline (anonymous, TTL) listeners: decide explicitly (non-goal) | §3.21 | P2 | Low |
| T19 | Application emoji registry with per-environment ids | §5.6 | — | Low |
| T20 | Event Handler identity must be unique (no timestamp ids) | §3.13 | bug | Low |

---

## A. tescord bugs and footguns the PRD does not yet rule out

Already covered by the PRD (no action): O(n) dispatch (FR-15), middleware context
never reaching handlers (FR-20), first-occurrence-only placeholders (FR-42),
unescaped separators / no 100-char check / lossy JSON fallback (FR-16), silent
dropping of base options when subcommands exist (FR-9), first-wins base metadata
(FR-9), missing `integration_types` (FR-11), dbi's unbounded refs (FR-38 bounded
defaults), dbi silent truncation (FR-16, FR-17 overflow policy).

### T1. Duplicate Command Paths across Modules (High)
tescord: "When several combinations share a sub name, the first registration wins."
FR-9 covers conflicting metadata and options-vs-subcommands, and FR-15 covers
duplicate Routes, but no FR covers two handlers for the same Command Path (directly,
or via overlapping Command Patterns).

> **FR-T1: Unique Command Paths.** Registration fails with a typed error naming both
> Modules when two registrations (including expanded Command Pattern combinations)
> resolve to the same Command Path, or to the same context-menu name and type.

### T2. Base and group descriptions (High)
tescord takes the base description from the first non-empty registration and copies
the first member's description to subcommand groups. Order-dependent metadata is the
same class of bug FR-9 already rejects.

> **FR-T2: Explicit base and group metadata.** A Module can declare the description
> (and its localizations) of a base command and of a subcommand group independently
> of any leaf Command. If none is declared, Publish uses a documented deterministic
> rule (not registration order) and logs a warning; if two Modules declare different
> values, registration fails with a typed error.

### T3. Option ordering (High)
tescord does not enforce required-before-optional ordering, so Discord rejects the
publish. FR-10 should catch this locally.

> **FR-10 additional consequence:** A required option declared after an optional one
> is a compile-time error for derived option structs and a typed registration error
> for dynamic declarations. Choices and autocomplete on the same option are mutually
> exclusive (compile-time error).

### T20. Unique Event Handler identity (Low)
tescord ids are `${packId}:${event}:${Date.now()}`; two registrations in the same
millisecond collide.

> **FR-6 additional consequence:** Registering two Event Handlers for the same event
> in the same millisecond, or from the same Module, keeps both; handler identity
> never depends on time.

## B. tescord/dbi features that should become FRs or consequences

### T4. Components built only from registered Routes (High)
tescord's `buildComponent({id})` fails at runtime when the id is missing, and
`@tescord/type-gen` exists purely to type-check ids. The PRD's FR-16 types the
Payload but does not say a component can't be built for a Route with no handler.

> **FR-T4: Route-bound component construction.** A component is built from its Route's
> typed handle, so emitting a component whose Route has no registered handler is a
> compile error, or, for dynamically composed Modules, a typed error at App startup.
> No code-generation or watch step is required.

### T5. Route defaults, overrides and modal pre-fill (High)
schwi-v2 uses `buildComponent({ id, data, overrides })` everywhere: the Route
registers default options (style, label, emoji), each render overrides some
(`disabled`, localized `label`), and modals are pre-filled (`value`) via overrides.
The PRD has builders (FR-23) and Routes (FR-15) but no link between them, and FR-29
does not mention pre-filled values.

> **FR-T5: Route component defaults.** A Route can declare default presentation for its
> component (style, label, emoji, placeholder, min/max values). Each render can
> override any of them, and the result is still Limit-validated.
> **FR-29 additional consequence:** A modal can be opened with pre-filled values for
> Text Inputs and default selections for selects, e.g. from the entity being edited.

### T6. Stale Payloads after a deploy (High)
schwi-v2 messages live for months; buttons on them carry Payloads encoded by older
code. The research proposes a versioned codec. The PRD covers expired Refs (FR-18)
and malformed ids (FR-19) but not "valid id, old Payload shape".

> **FR-T6: Payload evolution.** When a Payload was encoded by an incompatible earlier
> version of its type, Noctis routes the interaction to the Expired Handler (or a
> dedicated Stale Handler) instead of a generic error. Additive changes documented as
> compatible (e.g. a new optional field) decode successfully.

### T7. Ref namespacing across Apps (High)
dbi refs are "not namespaced per client/shard". FR-47 allows several Apps to share
Adapters; a Ref created by App A could then be resolved by App B's Route with the same
name.

> **FR-17 additional consequence:** Refs are namespaced by App. A Ref created by one App
> is never resolved by another App sharing the same RefStore, and is treated as
> missing (Expired Handler).

### T8. Autocomplete failure behaviour (Med)
tescord: on error → `tescord:autocompleteError` and respond `[]`; no handler →
respond `[]`. FR-10 covers only truncation; FR-22 has one generic sink.

> **FR-10 additional consequences:** If an autocomplete handler errors or exceeds the
> response deadline, Noctis responds with an empty choice list and routes the error to
> the autocomplete error sink; the user never sees "loading options failed" caused by
> a missing response.
> **FR-22 additional consequence:** Error sinks are distinguishable by origin
> (Command, Component/Modal, Autocomplete, Event, Render) so an author can handle
> each differently.

### T9. Rate-limit parity with dbi (Med)
dbi supports `User | Channel | Guild | Member | Message` scopes, dynamic
`ctx.setRateLimit(type, ms)` inside the handler, and an `interactionRateLimit` hook to
customise the response. schwi-v2 hand-rolls sliding windows (e.g. channel renames,
2 per 10 min). FR-46 lacks Message scope, dynamic limits, multi-hit windows and a
custom response.

> **FR-46 extension:** Rate limits support the scopes user, member, channel, guild and
> message; a limit is "N uses per window", not only one use per cooldown. A handler can
> set or extend a limit dynamically (e.g. only after a successful action). The
> rate-limited response is overridable per App or per Command/Route; the default is the
> localized notice with time remaining.

### T10. Boot-time feature flags (Med)
dbi `flag: "debug"` + `load("all")`; schwi-v2 `--features all|seacord`. One binary runs
as main bot, dedicated-server bot or white-label bot. FR-45 covers runtime toggles
per guild but not selecting which Modules/registrations exist at boot (which also
changes what Publish sends).

> **FR-T10: Boot-time flags.** A Module or individual registration can carry one or more
> flags. The App selects active flags at startup from configuration or CLI (e.g.
> `--features a,b` / `all`). Unselected registrations are not registered, not
> dispatched and not published.

### T11. Lifecycle hooks (Med)
tescord exposes `clientReady/clientsReady`, `interactionsPublished/PublishError`,
`pack:loaded/unloaded`, `cacheRefreshed`, `localesRefreshed`. The PRD has Event
Handlers for gateway events only.

> **FR-T11: Lifecycle hooks.** A Module can subscribe to framework lifecycle events:
> App started, Shard ready/resumed/disconnected, all Shards ready, Publish completed or
> failed (with the diff), Module enabled/disabled (global or per guild), and shutdown
> started.

### T12. Locale details (Med)
- tescord maps a base language file to every Discord variant (`en` → `en-GB`,
  `en-US`; `es` → `es-ES`, `es-419`) at Publish, and maps `pt-BR` → `pt` at runtime.
- User Locale falls back to Guild Locale, then default; events have only a Guild
  Locale (extracted from the event).
- Each Pack contributes its own locale files, merged per language.
- schwi-v2 rule: User Locale for ephemeral single-user UI, Guild Locale for shared
  content.

> **FR-41 additional consequence:** A Locale file for a base language (`en`) covers all
> Discord variants of it (`en-US`, `en-GB`) at Publish unless a variant file exists;
> regional files override base files.
> **FR-43 additional consequences:** Resolution order is User Locale → Guild Locale →
> default Locale for interactions, and Guild Locale → default for Event Handlers on
> guild events. Context-menu names are localized as whole names.
> **FR-T12: Module-scoped locales.** Each Module can ship its own Locale files; they
> merge per language with the App's, and a key defined by two Modules is a build-time
> error.
> Usage Skill: document the user-vs-guild locale rule.

### T13. Multi-client model (Med)
tescord runs several clients over one shared handler tree, with a per-command
`clientId` publish target; schwi-v2 additionally runs per-customer white-label bots
that only serve one guild, and the main bot skips those guilds. FR-47 says "several
Apps with different tokens" but not whether they share Modules, or how commands
target an App.

> **FR-47 additional consequences:** The same Module tree can be mounted on several
> Apps. A Command can be restricted to a subset of Apps for Publish. An App can be
> given a guild allow-list or deny-list (resolved through an Adapter), so a white-label
> App serves only its customer's guild and the main App skips it.

### T14. Payload extensibility (Med)
tescord's `customData:encode/decode` hook (P0 "as trait") lets apps encode their own
types (e.g. "database id → entity"). FR-16 types Payloads but does not say authors
can add field types.

> **FR-16 additional consequence:** A bot author can make their own type usable as a
> Payload field by implementing a Noctis trait; the round-trip property test and
> 100-character checks apply to it.

### T15. Publish extras (Low)
dbi `publish(..., clear)` sends an empty array; tescord publishes per client.

> **FR-13 additional consequences:** Publish can remove all of an App's commands from a
> guild or globally (`--publish-clear`). With several Apps, Publish runs per App.

### T16. Fallback / prefix routing (Low, P2)
tescord Inspectors (fallback routers with wildcard ids; wildcard never implemented).

> **FR-T16 (P2, may defer):** A Module can register a fallback handler for unmatched
> Routes and Command Paths within its scope, or a Route prefix handler. Default is the
> FR-19 typed error.

### T17. Option validators (Low, P2)
dbi `validate` hooks on options.

> **FR-10 optional consequence:** An option can declare a validator; a failed validation
> returns a localized ephemeral message and the handler does not run.

### T18. Inline listeners (Low, decision)
dbi inline TTL listeners are rated P2 by research but contradict NFR-2 and the brief's
doctrine.

> Add to §7 Non-Goals: "No anonymous/inline per-message listeners; one-off confirm
> dialogs use a Route with a one-shot Ref (FR-17)."

### T19. Emoji registry (Low)
schwi-v2 keeps dev/prod custom-emoji id tables selected at startup.

> **FR-T19 (optional):** A bot author can declare named emojis with per-environment ids
> (or Discord application emojis) and reference them by name in components and
> Templates.

## Items checked and found consistent

Root object + default language (FR-1, FR-41); Packs → Modules with path-like ids
(FR-44); hot unload → runtime enable/disable (FR-45, §7); pattern names and BFS
semantics (FR-8); cross-Module merge (FR-9); typed options, choices, channel types,
autocomplete truncation (FR-10); permissions/contexts/nsfw/integration_types (FR-11);
context menus (FR-12); publish CLI mode and guild publish (FR-13); typed Payloads +
RefStore with TTL, one-shot, expired path (FR-16–18); middleware before/after with
typed Extensions (FR-20); typed content strings with all-occurrence placeholders
(FR-42); ordered events opt-in (FR-6); generic KV store (CacheStore, FR-38); sharding
in-process (FR-2/3); `broadcastEval` deferred (§8.2); translation scaffold deferred
(§8.2); CV2 typed builders (FR-23); render-function = View pattern (FR-25);
pagination helper (FR-28); images (satori → takumi, FR-32–37).
