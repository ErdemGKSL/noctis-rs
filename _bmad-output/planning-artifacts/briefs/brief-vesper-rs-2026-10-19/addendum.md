---
title: "Product Brief Addendum: Vesper"
created: 2026-10-19
updated: 2026-10-19
---

# Addendum: detail for the PRD and architecture

The user supplied this detail. It is too specific for the brief, so it is kept here for
the PRD and architecture work.

## UI doctrine (from the author's original guidance, translated)

- **The two primary surfaces are Components V2 and image generation.** Generated
  images (takumi) give a clean, dynamic, read-only view. Every interactive or
  detailed, dashboard-like surface uses Components V2.
- **The framework documents Components V2 limits and allowed components one by
  one.** It also encodes the rules for writing high-quality interfaces. When a view
  exceeds the component limit, it paginates.
- **Slash commands are mostly bootstrap entry points.** For example, `/setup start`
  takes no parameters and replies with one large container that has navigation
  inside it.
- **Setup-style flows split content logically, not by page.** Prefer steps with a
  progress indicator over pagination.
- **General-purpose views that don't fit one container use a navbar.** A row of
  buttons sits at the top, and clicking one swaps the container's content.
- **There is never a listener per component.** Each component id has one global
  listener. Extra data goes after the base id, separated by a special character.
  When the data is large, a reference id goes there instead, and the data is looked
  up in a HashMap-like store and returned to the user.
- **Memory allocation stays small.** The framework uses allocation-conscious
  algorithms throughout, and everything is designed for a bot serving millions of
  users.

## Adapter principle (applies project-wide)

- **Storage-like concerns are traits with read/write functions.** The reference
  store is the canonical example. The framework ships a default implementation
  (in-memory), and the developer can implement the trait on their own struct
  (database-backed, Redis, …) and register it with the framework.
- **The principle is not limited to the reference store.** Apply it everywhere it
  makes sense: caches, render caches, locale sources, rate-limit buckets and similar.

## Platform choices

- **Runtime and gateway:** twilight-rs on tokio, multi-threaded, with sharding
  included, following twilight's recommended gateway structure.
- **Syntax:** follow tescord (the successor to `@mostfeatured/dbi`, as used in
  `schwi-v2`) as closely as practical. Features matter more than identical output.
  Examples include the space-separated command name split into command, group and
  subcommand, and auto-publish.
- **Images and GIFs:** generated with takumi, with built-in Tailwind CSS support.

## Research inputs

`../../research/`:
- `tescord-features.md`
- `twilight-rs.md`
- `discord-components-v2.md`
- `takumi-image-generation.md`
