# Application emojis

An application emoji is a small image the bot's application owns. The bot can place it
in a line of text, on a button or on a select option, in every server and DM, without
the `USE_EXTERNAL_EMOJIS` permission. Noctis treats them as part of the code: the set
is declared in Rust, uploaded by Publish, and looked up by a typed name from every
handler context. A bot never writes upload code and never keeps a table of emoji ids.

This guide covers declaring a set, the sync policies, Clusters, several applications
(a test bot and a production bot), and testing. The reference for every item named
here is the Usage Skill's
[images-emoji.md](../../examples/project-template/.claude/skills/noctis/references/images-emoji.md)
(§3 declaring, §4 Publish, §5 lookups, §2 the methodology for the art); the Project
Template uses all of it (`src/emoji.rs`, the dashboard's Modules tab, the rank
feature).

## The model in one paragraph

Declaring a set is compile-time data: names, files, formats and sizes are checked at
`cargo check`. **Publish** is the only writer: after it publishes the Commands, it
lists the application's emojis and creates every declared name that is missing. Every
process, the publisher included, **lists** the emojis at start (read-only) and builds
a name → id table that `ctx.emojis()` reads with no I/O. A name that is still missing
renders as no emoji, never as an error, and the table re-lists until it appears.

## Declaring a set

```rust
use noctis::prelude::*;

#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "assets/emoji", art = icon_art)]
pub enum Icons {
    #[emoji(white)]
    Check,        // check.png and check_white.png
    #[emoji(white)]
    Cross,
    StOnline,     // st_online.png
    #[emoji(render)]
    LevelUp,      // drawn by `icon_art` at Publish
}

fn icon_art(_icon: Icons, _tone: EmojiTone) -> EmojiArt {
    EmojiArt::template(LevelUpGlyph { tint: 0x58_65F2 })
}
```

Register it with `builder.emojis::<Icons>()`. The rules:

- **One variant per icon.** The variant is the emoji name in `snake_case` (`StOnline`
  is `st_online`); `#[emoji(name = "..")]` overrides it. Names are an API: messages
  already sent carry the name and the id.
- **Files** live in `dir` (default `assets/emoji`) as `<name>.png`, `.gif` or `.jpg`
  and are embedded in the binary. A GIF is an animated emoji.
- **White pairs.** `#[emoji(white)]` also declares `<name>_white` from
  `<name>_white.png`. `ctx.emojis().on(style, icon)` picks it on `Primary`, `Success`
  and `Danger` buttons and the coloured variant everywhere else, so a coloured icon
  never sits on a coloured button.
- **Drawn icons.** `#[emoji(render)]` reads no file: the set's `art` function returns a
  Template, and Publish renders it through the App's Renderer (PNG, or GIF when the
  Template animates, at most 256 KiB). Keep a pixel snapshot of the art Template so a
  change of the art is a reviewed diff.
- **Checked at `cargo check`:** a bad or duplicate name (`[EMO-03]`), a missing file or
  one that is not PNG, JPEG or GIF (`[EMO-04]`), a file over 256 KiB (`[EMO-02]`).
  **Checked at `build()`:** a name two registered sets declare (`[EMO-03]`) and more
  than 2000 names (`[EMO-01]`). The rules and their sources are in
  [discord-rules.md](../discord-rules.md).

The art follows the methodology of the Usage Skill (images-emoji.md §2): one vector
style for the whole set, exported as PNG with a transparent background, a white and a
coloured variant of every icon used on buttons, white on non-Secondary buttons. An
image-generation tool draws illustrative icons well; geometric glyphs are exact when
the Noctis image engine draws them.

## Publish and the sync policies

`cargo run --release -- --publish` (or `--publish-guild <id>`, or the process started
with `NOCTIS_PUBLISHER=true`) syncs the set after the Commands, for both scopes; a
`--publish-clear` leaves emojis alone. The sync:

1. lists the application's emojis (`GET /applications/{id}/emojis`);
2. prepares every image first (files as they are, drawn icons rendered), so a render
   failure or the 2000-emoji guard fails the Publish before anything is written;
3. writes one request at a time through the App's HTTP client and rate limiter:
   creates for missing names, and the opt-in deletes and replacements below;
4. re-lists, so the publishing process knows the new ids at once.

What happens to an emoji that already exists:

| Situation | Default | Opt-in |
| --- | --- | --- |
| Declared and present, same image | unchanged | — |
| Declared and present, image changed | reported as `changed`, kept | `EmojiSync::new().replace_changed(true)`: deleted and re-created |
| Present but not declared (uploaded by hand, or a removed variant) | never touched | `EmojiSync::new().delete_undeclared(true)`: deleted |

Set the policy with `builder.emoji_sync(EmojiSync::new().replace_changed(true))`.
Replacing gives the emoji a **new id**: messages already sent keep the old
`<:name:id>` token, which then shows as plain `:name:` text. When old messages matter,
give a redesigned icon a new variant instead.

A changed image is detected by a blake3 digest of the uploaded bytes, stored with the
emoji id in the App's `Kv` (Discord returns no hash and re-encodes images). An emoji
with no stored digest, uploaded by hand or published before the store existed, is
adopted as unchanged. With the in-memory default store, digests are lost at restart, so
`replace_changed` only sees changes when the App has a durable `CacheStore` adapter.

`--dry-run` lists and prints the plan under `== emojis ==` without writing.
`PublishReport::emojis()` (and the `PublishCompleted` hook event) carries the
`EmojiReport`: created, unchanged, changed, replaced and deleted names. A failed sync
is safe to repeat: the Commands are already published, and the next Publish skips the
names created before the failure. The errors are `PublishError::EmojiNeedsRenderer`
(a drawn icon on an App without a Renderer), `EmojiArt`, `TooManyEmojis` and
`EmojiRejected` (a 400 on create).

## Lookups

`ctx.emojis()` works in every context (commands, components, modals, autocomplete,
event handlers, hooks) and `app.emojis()` on a built App. Pass it to a View like the
strings:

| Call | Returns | For |
| --- | --- | --- |
| `on(style, icon)` | `Option<Emoji>`: white on `Primary`, `Success`, `Danger` | Route buttons |
| `get(icon)` | `Option<Emoji>`, the coloured variant | select options, Link buttons |
| `text(icon)` | a `Display` token, `<:name:id>`, empty while missing | text |
| `missing()`, `is_loaded()`, `reload()` | the table's state, a forced re-list | status screens, after a hand upload |

A missing emoji is never an error: the lookups return `None`, the token writes nothing,
and the component keeps its label. Start-up logs the missing names once (target
`noctis::emoji`) and tells you to run Publish. A failed list at start only warns: the
bot starts with an empty table and retries with backoff (1 s doubling to 60 s).

## Clusters

A Cluster needs no coordination for emojis, because the roles are already split:

- **Only the publisher writes.** Exactly one process publishes (the deploy step that
  runs `--publish`, or the one process with `NOCTIS_PUBLISHER=true`), so exactly one
  process creates, replaces or deletes emojis. There is no lock and no extra port.
- **Every other process re-lists.** A process lists at start and then re-lists every
  60 s while a declared name is missing
  (`AppBuilder::emoji_reload_interval(Duration)`; `Duration::ZERO` turns it off), and
  stops once none is. After the publisher's upload, the other processes pick up the
  new ids within a minute, without a restart.
- **Digests are shared through the `Kv`.** When `replace_changed` is on, give the
  Cluster one shared `CacheStore` adapter (the [Cluster guide](cluster.md), "Shared
  Adapters"), so the publisher of the next deploy sees the digests of the last one.

Start order does not matter: a process that starts before the first Publish runs with
missing emojis (and says so in its log) and heals on its own.

## Environments with different ids

Emoji ids differ per application. A test bot and a production bot built from one
source, or the several Apps of a white-label process, each have their own ids:

- **Never write an id in code.** `ctx.emojis()` resolves names against the
  application of the App's token, and every App in a process has its own table.
- **Publish each application once.** The first Publish of a new application uploads
  the whole set; later Publishes create only what is missing.
- **The literal form is for single-application bots.** A
  `defaults(emoji = "<:check:1300000000000000001>")` literal on a Route pins one
  application's id; set the emoji per render with `.emoji(e.on(..))` instead. Emoji
  names in `defaults(..)` are not supported, because defaults are compile-time data
  built without a context.

## Testing

- `TestApp` resolves every declared name to a stable fake id,
  `noctis::testkit::fake_emoji_id(name)`, without a request. Assert the exact token
  (`format!("<:level_up:{}>", fake_emoji_id("level_up"))`) or find a button by its
  emoji (`response.button_by_emoji("check")`, which also matches `check_white`).
- `app.app_emojis(["check"])` keeps only the names given, which tests the degraded
  path: the other lookups are `None` and their tokens write nothing.
- A pure render function takes `&Emojis`; in a unit test, `app.emojis()` of an App
  built with `configure(&mut App::builder()).build()?` is a table that never listed,
  so every emoji is missing.
- Keep a pixel snapshot of every drawn icon's Template
  (`assert_render_snapshot!(app.renderer(), "emoji_level_up", ..)`).
- Before the first production Publish, run
  `cargo run --release -- --publish-guild <test guild> --dry-run` against the test
  application and read the `== emojis ==` plan.

The Project Template's `tests/flows.rs` (the dashboard's Modules tab) and
`tests/rank.rs` (the leaderboard heading and the level-up line) show each of these.
