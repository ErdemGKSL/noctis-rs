---
title: "Noctis: maintainer decisions"
updated: 2026-10-20
---

# Maintainer decisions (binding for every story)

These are decisions the maintainer made in conversation. Each one is already written
into the PRD, the architecture spine, `epics.md`, `docs/discord-rules.md` or the Usage
Skill. This page gathers them in one place so that planners, implementers and
reviewers apply them consistently.

When an older spec, story text or skill passage contradicts this page, this page wins.
Fix the other document in the same change.

## 1. UI: Noctis is a Components V2 wrapper, not a widget framework

- **Dropped widgets.** There is no Navbar/tabs widget, no Wizard, no Paginator, no
  widget state, no `view` envelope or `#[route(.., view)]`, no automatic re-render and
  no automatic overflow pagination. FR-26, FR-27, FR-28 and AD-20 are withdrawn.
- **Views are plain functions.** A View is `fn render_x(..) -> message`, written by the
  bot author. The slash command and every component and modal handler call the same
  function. State such as ids, page and section lives in ordinary typed Payload fields
  of the module's own Routes.
- **What Noctis provides:**
  - typed components and placement rules;
  - compile-time and runtime limit validation;
  - `cost()` and "how many fit" budget helpers;
  - Route-built components with length-proven `custom_id`s;
  - Responder ergonomics (`ctx.respond`, `ctx.update`, `ctx.reply_text`).
- **Recipes, not features.** Tabs, pagination, step flows and settings screens are
  recipes in the Usage Skill (`views-and-recipes.md`, "Best practices (schwi-style)").
  They follow the maintainer's schwi-v2 bot.
- **Story 6.10** is the last story of Epic 6. It removes the widget code that Epics 1–6
  already shipped. Epic 7 onwards assumes the simplified codec.

## 2. Interaction responses follow discord.js exactly

These semantics were verified against discord.js 14.27.0 `InteractionResponses`.
Each method is a distinct flow:

| Method | Callback / route | Effect |
| --- | --- | --- |
| `reply(msg)` | type 4 | new message; ephemerality is fixed here |
| `defer_reply(eph)` | type 5 | new "thinking…" message |
| `update(msg)` | type 7 | edits the clicked component's message in one request |
| `defer_update()` | type 6 | no visible change |
| `show_modal(m)` | type 9 | opens a modal |
| `edit_reply` / `fetch_reply` / `delete_reply` | `PATCH` / `GET` / `DELETE …/messages/@original` | acts on the reply, or on the component's message after `update`/`defer_update` |
| `follow_up(msg, vis)` | `POST /webhooks/{app}/{token}` | another new message |

- **Equivalences.** These are tested on the wire:
  - `defer_update` + `edit_reply` ≡ `update`;
  - `defer_reply` + `edit_reply` ≡ `reply`.
- **Where `update` works.** `update` and `defer_update` exist only for components and
  for modal submits whose modal was opened from a component message. Any other modal
  submit gets `UpdateUnavailable` and no request is sent.
- **`show_modal` comes first or not at all.** It is impossible after any ack: `reply`,
  `defer_reply`, `update`, `defer_update` or an auto-defer.
  - Handlers declared `opens_modal` have no auto-defer, so they must open the modal
    within 3 s.
  - The typestate removes the method after an ack.
  - A runtime race returns `ModalUnavailable` and sends no request.
- **Updates never change ephemerality.**
  - A legacy message converts to V2 by setting the flag and sending `content: null`
    and `embeds: []`.
  - Converting V2 back to legacy is a compile error.
- **Interaction requests skip the bot token.** Interaction callbacks and
  interaction-webhook requests carry no bot `Authorization` header and never send
  `with_components`.
- **Expired tokens.** After the token expires (15 min), public messages fall back to
  the bot-token channel routes; everything else returns `TokenExpired`.
- **Renames.** Story 6.10 renames the shipped command methods to these names, with no
  aliases. Story 7.2 specifies the full matrix.

## 3. HTTP: a warm, deadline-safe Discord edge (Story 5.9)

- **Edge.** twilight-http is pointed at a loopback edge through its public `proxy(..)`
  setting. The edge owns a tuned hyper client:
  - HTTP/2 PING every 20 s with a 5 s timeout;
  - TCP keepalive and nodelay;
  - a connect timeout;
  - async DNS cached for its TTL;
  - a connection pre-warmed at startup and after every reconnect.
- **No API polling.** Nothing calls `/users/@me` or any other endpoint just to keep a
  connection warm.
- **Callbacks.** Interaction callbacks bypass twilight's RateLimiter. A stalled callback
  is retried (hedged) on a fresh connection after a time budget, and Discord's 40060
  "already acknowledged" counts as success.
- **Auto-defer at 2000 ms.**
- **Cluster proxy.** A configured Cluster HTTP proxy is the edge's upstream; the two
  compose rather than conflict.

## 4. Discord rules: enforce at compile time first

- **Enforcement order.** Compile time (types, const asserts, proc macros) → App build
  (rules that need the combined Modules) → runtime (genuinely dynamic data only).
- **Length-proven `custom_id`s.** Refs have a fixed length of 11 base62 characters and
  Payload fields are bounded, so every Route's maximum `custom_id` length is proven at
  compile time.
- **Command Patterns.** They are expanded and validated in the macro, so
  `config (set|get) (prefix|welcome)?` is a compile error naming `CMD-11`.
- **Rule ids.** Every rule has an id in `docs/discord-rules.md`, used in code, error
  messages and tests.
- **Placement.** The placement matrix (messages, Containers, Sections, Action Rows,
  modals, Labels) is enforced by sealed traits and documented in the Usage Skill.

## 5. Project template and Usage Skill

- **Single binary.** The template ships as one self-contained binary:
  - locales come from `i18n!`;
  - the theme comes from `theme!()`;
  - fonts come from the `default-font` feature;
  - `.env.example` is embedded with `include_str!`.

  On first run the binary writes `.env` with owner-only permissions and a fresh
  payload key, then exits.
- **Memory.** Embedded data that is used once is never copied to the heap. Buffers
  built from it, and every secret, are `zeroize::Zeroizing`. Locale data stays
  resident.
- **Agents learn only from the skill.** Bot authors' agents learn Noctis from the Usage
  Skill and never from the source. Every public API change updates the template, the
  skill and the API snapshot in the same commit.
- **Tailwind is native only.** Templates use takumi's parser through Noctis's own
  strict class grammar. There is no Tailwind CLI and no Node.js.

## 6. Engineering workflow

- **Release builds only.** Every build, test, check and run uses `--release`. trybuild
  is covered by the repository's release-equivalent `[profile.dev]` in
  `.cargo/config.toml`, which must never be weakened.
- **Disk.** Nothing is cleaned automatically. `target/` is kept for fast incremental
  builds and is cleaned only when the disk is actually full.
- **Commits.** The story loop commits and pushes at every step (plan, start, review
  pass, done, epic done) to `main` and the session branch. Nobody commits half-finished
  story work; the git stop hook is disabled for that reason.
- **README.** The README is refreshed after every third epic (3, 6, 9, 12, 15).
- **Crates.** Noctis is published as the facade crate `noctis`, with `noctis-*`
  sub-crates behind its features. It is licensed MIT OR Apache-2.0, and `cssparser`'s
  MPL-2.0 is accepted.
