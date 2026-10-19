# Reconciliation: PRD vs `research/discord-components-v2.md`

**Date:** 2026-10-19. **PRD:** `prd.md` (draft). **Input:** `../../research/discord-components-v2.md` (docs snapshot `f7416a2`, 2026-10-16).
Scope: capabilities and constraints the PRD misses or states wrongly. Pure implementation detail (type layouts, builder method names, encodings) is left to architecture.
Severity: **High** = PRD is wrong or a story built from it would ship a Discord 400 / broken UX; **Med** = missing requirement with user-visible effect; **Low** = clarification.

---

## G-1 (High) FR-29 modal component set is incomplete and wrong in one detail

**Research:** Modal Label children are Text Input, String/User/Role/Mentionable/Channel Select, File Upload, **Radio Group (21)**, **Checkbox Group (22)** and **Checkbox (23)** (§1.14). Checkbox can't be `required` (use a one-option Checkbox Group). Each Label + child costs 2 components; modals allow 1-5 top-level components (Label or Text Display). File Upload submissions resolve to **ephemeral CDN URLs** that must be downloaded promptly. `required` defaults to `true`, and `min_values` must be omitted or ≥ 1 when `required` is true. Text Input `label` is deprecated; only Label-wrapped inputs should be emitted.
**PRD:** FR-29 lists "Label, Text Input, select menus, File Upload, Checkbox and Text Display". Radio Group and Checkbox Group are missing. No rule on File Upload URL lifetime or on `required`/`min_values`.

**Suggested FR-29 text:**
> A bot author can build modals whose top level is Label or Text Display, and whose Labels wrap Text Input, any select menu, File Upload, Radio Group, Checkbox Group or Checkbox. They receive the submission as a typed struct, with selects joined to `resolved` entities and File Upload values joined to `resolved.attachments`.
> **Consequences (testable):**
> - The builder rejects `disabled` anywhere in a modal, more than 5 top-level components, title > 45, Label `label` > 45 / `description` > 100, Radio Group options outside 2-10, Checkbox Group options outside 1-10, `required` on a Checkbox, and `min_values = 0` combined with `required = true`.
> - Noctis never emits a deprecated Action Row-wrapped Text Input or a Text Input `label`.
> - File Upload values expose the attachment metadata and a helper to download it; the docs state that the URL is ephemeral.

## G-2 (High) Modal-opening handlers conflict with FR-21 auto-acknowledgement

**Research:** `MODAL` (9) must be the **first** response; it can't follow a defer, and it can't be sent in response to `MODAL_SUBMIT` (no modal chaining). A multi-step modal flow must go modal → message ("Continue" button) → modal (§3.1, §9.7).
**PRD:** FR-21 defers automatically after ~2.5 s with no exception. A slow handler that intends to open a modal would be deferred and then fail. FR-27 (Wizard) doesn't mention the no-chaining rule.

**Suggested addition to FR-21:**
> - A handler declared as modal-opening is never auto-deferred. If it hasn't opened the modal before the deadline, Noctis logs an error and the interaction fails visibly to the author in logs, not silently.
> - Responding to a modal submit with another modal is a typed (preferably compile-time) error.

**Suggested addition to FR-27:**
> - A Step that collects input through a modal returns to a message Step (UPDATE_MESSAGE) before the next modal; Wizard navigation handles this automatically.

## G-3 (High) FR-21 deferred-response rules are only half stated

**Research (§2.3, §2.4):**
- `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` (5) accepts **only `EPHEMERAL`**; the V2 flag goes on Edit Original. The PRD gets this part right.
- **Ephemerality is fixed at creation.** The decision to be ephemeral must be made at defer time, so an auto-defer must already know whether the eventual reply is ephemeral.
- Component interactions defer with `DEFERRED_UPDATE_MESSAGE` (6), not 5; types 6/7 are valid only for component and modal-submit interactions.
- Interaction tokens last **15 min**; edits after that need the bot token (`Edit Message`), which only works for non-ephemeral messages.
- User-installed apps not installed in the guild get **at most 5 follow-ups** per interaction.

**Suggested FR-21 consequences:**
> - Each Command and Route declares (or defaults) whether its reply is ephemeral; the auto-defer uses that flag, because ephemerality can't change after the first response.
> - For component and modal-submit interactions the auto-defer is `DEFERRED_UPDATE_MESSAGE`; for commands it is `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` with only `EPHEMERAL`, and the V2 flag is added on the edit.
> - After the 15-minute token lifetime, updates to a non-ephemeral View fall back to a bot-token message edit; updates to an ephemeral one return a typed `TokenExpired` error.
> - When the follow-up cap for user-installed contexts would be exceeded, Noctis returns a typed error instead of sending.

## G-4 (High) Webhooks are absent (`?with_components=true`)

**Research (§2.5):** Execute/Edit Webhook ignore `components` unless `?with_components=true`. Application-owned webhooks may send all components; non-owned webhooks only **non-interactive** ones. With V2, Execute Webhook rejects `content`, `embeds`, `files[n]`-style legacy usage and `poll`; Edit Webhook Message needs them nulled first. Allowed flags differ per endpoint.
**PRD:** No FR covers sending a View through a webhook (log channels, white-label personas, cross-posting are common in big bots).

**Suggested new FR (in 4.4):**
> **FR-x: Views over webhooks.** A bot author can send and edit a View through a webhook.
> - Noctis always adds `with_components=true`.
> - Sending a View with interactive components through a webhook the application doesn't own is a typed error.
> - Only flags valid for the target endpoint are sent (Execute: `SUPPRESS_EMBEDS`, `SUPPRESS_NOTIFICATIONS`, `IS_COMPONENTS_V2`; Edit: `SUPPRESS_EMBEDS`, `IS_COMPONENTS_V2`).

(Alternatively, list it explicitly in §8.2 Out of Scope.)

## G-5 (Med) V2 flag lifecycle and legacy messages

**Research (§2.1, §2.7):** The V2 flag can't be removed once set. A legacy message can become V2 on edit only if `content`/`poll` are set to `null` and `embeds`/`sticker_ids` to `[]`. Besides `content` and `embeds`, V2 forbids `poll`, `sticker_ids` and `shared_client_theme`. Legacy (non-V2) messages are not deprecated.
**PRD:** FR-23 forbids only `content` and `embeds`. It doesn't say what happens when a View update targets a legacy message (for example a message from FR-14 prefix commands, an old bot version, or the default error notice), or whether Noctis can send legacy messages at all.

**Suggested FR-23 consequences:**
> - `poll`, stickers and `shared_client_theme` can't be set on a V2 message either.
> - Editing a legacy message into a View clears `content`, `embeds`, `poll` and stickers in the same request.
> - Noctis exposes a minimal legacy (non-V2) send path for plain text, used by FR-14 and available to authors; a V2 message can never be edited back to legacy (typed error).

## G-6 (Med) FR-24 Limits list is incomplete; 4000-char budget is community-sourced

**Research (§0, §8.3):** Rules the validator must also enforce: Section 1-3 Text Displays plus exactly one accessory; Media Gallery 1-10 items; File component accepts only `attachment://`; link button `url` ≤ 512; premium button needs `sku_id` and forbids `custom_id`/`label`/`url`/`emoji`; Container `accent_color` within `0x000000-0xFFFFFF`; numeric `id` unique per message; select `default_values` count within `[min_values, max_values]`; select option label/value/description ≤ 100; alt text ≤ 1024. The 4000-char text budget is **[C]** (enforced by the API, not in Discord's docs), so it should be configurable.
**PRD:** FR-24 names labels, placeholders, option counts, media items and `attachment://` references only.

**Suggested FR-24 consequences:**
> - The validator also checks Section shape, gallery item counts, button style rules (link `url` ≤ 512, premium `sku_id` only), accent colour range, `default_values` count, and uniqueness of both `custom_id` and numeric `id` within the message.
> - All numeric Limits live in one configurable Limits value, so a Discord change (as with the 2025-04-29 raise from 10/30 to 40) needs no Noctis release.

## G-7 (Med) `allowed_mentions` default: Text Display mentions ping

**Research (§1.7, §6.7):** Mentions in Text Displays **ping** and are governed by `allowed_mentions`; dashboards should default to `parse: []` and escape user text.
**PRD:** Nothing. A leaderboard View listing `<@id>` would ping every listed user.

**Suggested NFR-6 bullet / FR-25 consequence:**
> - Views send `allowed_mentions` with no parsed mentions unless the author opts in per View or per message.
> - Noctis provides a helper that escapes Markdown and mentions in user-supplied text placed in a Text Display.

## G-8 (Med) Forward compatibility on receive (unknown component types)

**Research (§1.1, §8.1):** Discord warns that component lists and `accessory` may grow; types 15/16/20 are unassigned. Unknown types must deserialize as opaque values.
**PRD:** FR-31 covers outbound raw JSON only.

**Suggested FR-31 consequence:**
> - A received message or modal submit containing an unknown component type is still parsed; the unknown part is kept as raw JSON and is available to the handler. It never causes a dispatch failure.

## G-9 (Med) Re-rendered selects keep stale client state

**Research (§9.6):** After a re-render, the client keeps the user's local select choice unless the select's `custom_id` changes; schwi adds a nonce per render so `default` / `default_values` show the server state.
**PRD:** FR-25 says the same View renders every update but doesn't require that the shown state equals the server state.

**Suggested FR-25 consequence:**
> - After an update, every select shows the View's declared defaults, not the user's previous local selection. (Noctis varies select `custom_id`s per render within the 100-char budget.)

## G-10 (Med) `custom_id` is forgeable: authorization, not just decoding

**Research (§7, §9.8):** `custom_id`s are visible and forgeable by any client; authorization must be re-checked server-side in every handler. Optional truncated HMAC tags detect tampering of stateless ids.
**PRD:** FR-19 covers malformed input, not a well-formed but forged Payload (for example another user's id or a different guild's entity).

**Suggested FR-19 consequences:**
> - A Route can declare an owner/permission guard (e.g. "only the invoking user", "requires MANAGE_GUILD") that runs before the handler on every interaction.
> - Optionally, inline Payloads can carry a short integrity tag; a tag mismatch is treated as a forged Payload and routed like an unknown Route.

## G-11 (Med) Shared-dashboard concurrency

**Research (§6.9):** Two users clicking the same public dashboard → last write wins. Recommended: version in the `custom_id`/store and re-render from fresh state; ephemeral panels for per-user settings.
**PRD:** No requirement. UJ-2 dashboards are shared.

**Suggested FR-25 consequence:**
> - A View can opt into a state version; an interaction whose version is older than the stored one re-renders from current state instead of applying the stale action, and the user gets a short notice.

## G-12 (Low) Upload size comes from the interaction, not a constant

**Research (§0, §1.14, §2.6):** Default upload limit is 20 MiB per file (since 2026-09-03); the actual per-channel limit is `attachment_size_limit` on the interaction. Only `.jpg/.jpeg/.png/.webp/.gif` are documented for `attachment://` images; gallery also takes video.
**PRD:** FR-33 mentions "the upload size budget" without a source.

**Suggested FR-30/FR-33 text:**
> - The upload budget is the interaction's `attachment_size_limit` when present, otherwise a configurable default (20 MiB). Noctis rejects an attachment that exceeds it before sending.

## G-13 (Low) Accessibility: alt text

**Research (§6.6):** Always set `description` (alt text, ≤ 1024) on Thumbnails and gallery items.
**Suggested FR-24 consequence:** In strict mode, a Thumbnail or gallery item without alt text is a validation warning (debug) and is listed in the Limit cost report.

## G-14 (Low) Paginator detail

**Research (§6.2, §6.3):** Disabled "label" buttons (page indicator) still need unique `custom_id`s; > 25 select options must be chunked; for many pages a "go to page" modal or select is recommended.
**Suggested FR-28 consequences:**
> - Navigation reaches page N in a bounded number of clicks for large lists (jump control when pages > 5).
> - The Paginator can render items as a chunked String Select (25 options per select) as an alternative to one Section per item.

## G-15 (Low) Out-of-scope notes worth stating

- **Component Embeds** (link previews, 2026-10-05): same model, display-only, link buttons only. Add to §8.2 as deferred.
- **Legacy text fallback for DMs** (schwi pattern, §9.7): optional; mention as deferred or as part of G-5.
- Open questions in §10 already cover the 4000-char unit and per-Text-Display max. Add research Q3 (max media `url` length in messages; 2048 documented only for Component Embeds) and Q4 (whether `UPDATE_MESSAGE` from a modal submit can switch a legacy message to V2).
