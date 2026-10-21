# Epic 8 Context: Views as plain functions, Responder ergonomics and the testing kit

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author write every screen as a plain render function (`render_x(..) -> V2Message`) called from the Command and from every component and modal handler, answer with one call (`ctx.respond`, `ctx.update`, `ctx.reply_text`), size lists with budget helpers, test whole flows without Discord, and learn tabs, pagination, step flows and settings screens as recipes. Noctis ships no widgets and keeps no UI state; tab, page and step ids are typed Payload fields of the author's own Routes.

## Stories

- Story 8.1: Responder ergonomics: `ctx.respond`, `ctx.update` and `ctx.reply_text`
- Story 8.2: Deterministic renders, select freshness and safe text
- Story 8.3: Budget helpers: cost and "how many items fit"
- Story 8.4: Testing kit — simulated interactions and message snapshots
- Story 8.5: UI recipes and example: tabs, pagination, step flows and settings screens

## Requirements & Constraints

- No Navbar/Wizard/Paginator widgets, no widget state, no `view` envelope, no automatic re-render and no automatic overflow pagination (those features are withdrawn). Recipes, not features.
- `ctx.respond` picks initial response (type 4), edit of deferred original, or follow-up automatically; `ctx.update` edits in place (type 7, or `edit_reply` after type-6 auto-defer) and never falls back to a new message; `ctx.reply_text` sends a one-Text-Display notice, ephemeral by default, pinging nobody.
- `CommandCtx` has no `update` (compile error, INT-04). Modal submits opened from a slash command get `UpdateUnavailable` with no request sent. Second initial response through either API returns `AlreadyResponded` without a request.
- Renders must be deterministic: same state yields identical JSON except per-render select nonces. Select freshness is guaranteed: each send's `N` nonce never equals a nonce in the interaction being answered (clicked select, source message selects, scanned without allocating) nor one of the run's last eight sends, so clients show declared select defaults; by-Ref ids still mint a new Ref per send.
- `escape_markdown` / `escape_mentions` neutralize user text; mentions never ping unless opted in per message.
- Duplicate Route+Payload components in one message are a CID-02 error naming both paths with a help hint.
- Over-limit messages are a FR-24 error naming path, Limit and actual value; Noctis never splits/paginates; Lenient mode only truncates text.
- Framework memory must stay flat over N interactions (NFR-2).
- Every public API change updates the project template (code, AGENTS.md, Usage Skill: `responder.md`, `views-and-recipes.md`, `limits-cheatsheet.md`, testing reference), re-blesses the API snapshot, and passes `skill-check`.

## Technical Decisions

- Response semantics mirror discord.js exactly (reply/defer_reply/update/defer_update/show_modal/edit_reply/follow_up), with wire-tested equivalences `defer_update+edit_reply ≡ update` and `defer_reply+edit_reply ≡ reply`. The ctx methods sit on top of the typestate `Responder<K, S = Pending, D = AutoDefer|Manual>`, which remains the tool for modals and multi-message flows.
- Handlers declare `auto_defer` (default true) and `response = public|ephemeral|update` at registration; `show_modal` needs `auto_defer = false`.
- Every interaction context carries its run's shared responder state, so `ctx.respond`/`ctx.update`/`ctx.reply_text` and the typestate Responder share one gate: once the handler answered, Pending methods and `ctx.update` return `AlreadyResponded`, while `respond`/`reply_text` become follow-ups. After a type-6 defer a `ctx.respond` follow-up still allows a later `ctx.update`; after a modal or Middleware stop all three return `AlreadyResponded`.
- `reply_text` accepts `String`, `&'static str` or a `MessageKey` (rendered through the App's `Translate`); component contexts expose `response_locale()`.
- Interactions run through the fixed interaction pipeline; updates always set `IS_COMPONENTS_V2` and run the runtime validator.
- `noctis::ui::Budget::of(&msg, &limits)` reports components left (40 minus cost at every depth) and Text Display chars left (4000 minus counted text); `fits(&item)` gives max copies, `fits_prefix(iter)` counts in-order fits.
- `noctis::testkit` (facade `testkit` feature, dev-dependency): `TestApp` built on `Runtime::for_test()` with the existing fakes (`FakeDiscordHttp`), the bot's own builder installs plugins as in production, simulated `Inbound` interactions go through the gateway's lanes, no network. Snapshots use `insta` over rendered JSON. `assert_memory_flat` is added here.
- `skill-check` doctrine lint accepts `ctx.respond(`/`ctx.update(`/`ctx.reply_text(` and rejects raw Responder names on other receivers in interactive handlers; Story 8.5 extends it to `examples/`.

## UX & Interaction Patterns

- Tabs: `#[derive(PayloadEnum)]` tab field, `match` in render, Route button row with active tab disabled.
- Pagination: `page: u16` field, slice sized via `Budget`, prev/next disabled at bounds (prev/next distinguished by target-page Payload).
- Step flow: `step` field, "Step n of m: Title", back/next/cancel, per-step validation with inline notice; cancel leaves no components.
- Settings screen: Sections with accessory buttons, toggle, entity switcher select, add/delete rows, button → modal → re-render carrying the same Payload.
- Rulebook: one primary action per View, visible active state, one-shot Route confirmation for destructive actions, ephemeral for single-user UI / public for shared, a way back or cancel on every View, designed empty and error states, no per-message listeners.

## Cross-Story Dependencies

- Builds on Story 6.10 (Responder names, widget removal) and 6.11 (struct-per-interaction derives, select nonce), Epic 7 (Routes, Refs, modals), and Story 5.2/5.3 fakes.
- 8.3 Budget feeds the template dashboard Roles tab and 8.5 pagination; 8.4 testkit is used by 8.5's example tests; 8.1 ctx methods are used throughout 8.2–8.5.
- `examples/ui-recipes/` joins workspace `members` with `publish = false`.
