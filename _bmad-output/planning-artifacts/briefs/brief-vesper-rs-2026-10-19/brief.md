---
title: "Product Brief: Vesper"
status: final
created: 2026-10-19
updated: 2026-10-19
---

# Product Brief: Vesper

## Executive Summary

Vesper is a Rust framework for building Discord bots that look and feel like small
applications. It sits on top of twilight-rs and handles the parts every serious bot
rebuilds by hand: sharding across threads, command registration, interaction routing,
UI state, and dynamic images.

Discord's Components V2 lets a message act as a dashboard, with containers,
sections, galleries and buttons instead of plain text and embeds. Few bots use it
well, because the limits are strict and the state handling is tedious. Vesper makes
that UI the default. A slash command such as `/setup start` takes no parameters. It
opens a rich container with navigation, steps and live state. Generated images,
rendered with takumi and styled with Tailwind, show read-only data clearly.

The goal is to give Rust developers the ergonomics of tescord, the TypeScript
framework behind the author's schwi bot. Vesper should also run at the scale Rust
makes possible: millions of users on modest hardware, with memory use kept small by
design.

## The Problem

- **Raw twilight is a toolkit, not a framework.** It gives you a gateway, an HTTP
  client and models. Developers still write their own shard supervisors, command
  trees, publish scripts, custom-id parsers and handler dispatch for every bot.
- **Components V2 UIs are painful to build.** The limits are strict: about 40
  components per message, character budgets and nesting rules. Interactive flows
  need state that survives across clicks. Developers typically either cram data into
  `custom_id` strings until they hit 100 characters, or spawn a collector or
  listener per message. A listener per message leaks memory and breaks on restarts
  and across shards.
- **Rust lacks a high-ergonomics option.** Rust bot developers today pick between
  serenity/poise and raw twilight. Neither treats Components V2, UI navigation
  patterns or image generation as first-class. TypeScript developers using tescord
  or dbi get space-separated command names, auto-publishing, references in
  `custom_id` values, and locale and feature loading out of the box. Moving to
  Rust for performance means losing all of that.
- **Dynamic images are bolted on.** Generating status cards, profiles or leaderboards
  usually means a separate service (satori, a headless browser) with its own
  latency, memory and deployment cost.

## The Solution

A tokio-based framework, using twilight's recommended architecture, that provides:

- **Gateway and scale built in.** Sharding is configured up front. Shards run
  concurrently on a multi-threaded runtime, and the framework handles identify
  queueing, resumes and shard ranges for multi-process deployments. Events dispatch
  to handlers while avoiding per-event allocations wherever possible.
- **Tescord-style command authoring.** A command is declared with a name like
  `"settings logs channel"`. Vesper splits it into command, subcommand group and
  subcommand, builds the Discord payload (options, localizations, permissions,
  contexts), and publishes it automatically, globally or per guild, only when it
  changed.
- **Components V2 as the main UI.** Builders cover every V2 component, and the
  framework validates the limits before Discord does. It ships ready-made patterns:
  - step and progress wizards for setup flows;
  - a navbar of tab buttons that swaps the container content when a view doesn't
    fit;
  - automatic pagination when content exceeds component limits.
- **One handler per component id, never per message.** A `custom_id` is the
  component's base id, a separator, and either compact inline data or a reference
  id. A reference id points into a store that holds larger state.
- **Pluggable adapters everywhere.** Storage-like concerns are traits with a default
  implementation: the reference store, caches, render caches and similar. A
  developer implements, say, the read/write trait for their own database struct and
  passes it in. Memory, Redis and SQL become a choice, not a fork.
- **Built-in image and GIF generation.** takumi renders templates with Tailwind
  classes to PNG, WebP or GIF on a bounded render pool. The output attaches directly
  to V2 media galleries and thumbnails. This is an optional feature.

## What Makes This Different

- **UI-first.** Components V2 patterns (wizards, navbars, pagination, limit
  validation) and image generation are core features rather than examples or
  add-ons. No mainstream Rust framework does this today. `[ASSUMPTION]`: no
  competitor has shipped these since our research cutoff.
- **Stateless-by-default interaction model.** Routing by component id plus pluggable
  reference storage means handlers survive restarts and work across shards and
  processes. This is the architectural difference from collector-style libraries.
- **tescord ergonomics in Rust.** It is a familiar authoring model for developers
  coming from the TypeScript ecosystem, with Rust's performance and type safety.
- **Honest moat.** The advantage is execution and opinionated design, not a secret
  technology. Every piece builds on open crates (twilight, tokio, takumi). The value
  is in integrating them well and getting the defaults right.

## Who This Serves

- **Primary: the author and teams building large, UI-heavy bots.** These are
  community, moderation, game and utility bots that want dashboard-quality
  interactions and need to scale. Success means a new feature such as a setup flow
  or a stats card takes hours, not days, and the bot runs comfortably at millions of
  users.
- **Secondary: Rust developers coming from serenity/poise or from TypeScript
  frameworks.** They want batteries included without giving up control. Success
  means migrating a tescord or dbi bot feels natural.

## Success Criteria

- A bot with sharding, auto-published commands, a 3-step setup wizard and a generated
  image card fits in under ~150 lines of user code. `[ASSUMPTION]`
- Interactive state for a component never requires a per-message listener. All
  examples and docs use id-routed handlers with the reference store.
- At steady state with the in-memory store, the framework's own memory per idle guild
  stays within a documented, small budget. `[ASSUMPTION]`: this budget will be set in
  the PRD and measured by a benchmark in CI.
- Image rendering is non-blocking for the gateway and bounded under load, with
  documented p50/p99 render times for a standard card.
- The author's next production bot (the schwi successor) runs on Vesper.
  `[ASSUMPTION]`

## Scope

**In for v1**
- Gateway and sharding layer.
- Command tree from space-separated names, with options, autocomplete, localizations,
  permissions and contexts, plus auto-publish with diffing.
- Component and modal routing by id, with the custom-id codec.
- Reference-store, cache and render-cache adapter traits, with in-memory defaults.
- Components V2 builders with limit validation, plus wizard, navbar and pagination
  patterns.
- takumi image and GIF rendering with Tailwind.
- i18n for command metadata and responses.
- Events and feature modules.
- Examples and docs.

**Out for v1**
- Voice and music.
- Sharded-cache clustering beyond the adapter traits.
- Web dashboards.
- Prefix and message commands, unless research shows tescord users depend on them.
  `[ASSUMPTION]`
- Non-tokio runtimes.
- Hot-reloading code at runtime. Rust can't do it the way tescord does, so feature
  toggles replace it.

## Vision

Vesper becomes the default way to build "app-like" Discord bots in Rust. Its
adapter ecosystem includes crates such as `vesper-redis` and `vesper-sqlx`, a
library of reusable UI patterns and image templates, and tooling such as type
generation for locales and custom-id payloads. Teams that outgrow TypeScript
gain an order of magnitude in efficiency without losing ergonomics.
