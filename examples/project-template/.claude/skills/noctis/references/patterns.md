# Patterns, anti-patterns and common mistakes

Proven shapes for Noctis features, the shapes to avoid, and a table of the mistakes that
come up most often (symptom → cause → fix). Every pattern links the reference with the
complete, tested code; the snippets here are short and leave out imports (they come from
`noctis::prelude::*`, `noctis::ui::*` and your `crate::strings::{Strings, keys}`).
The `keys::*` names, types and helpers in the snippets (`NoteForm`, `BoardCard`, `top10`, …)
are placeholders: add each key to `locales/en-US.yaml` (and `tr.yaml`) and define the types
before use (in §8, `pager` is the `ActionRow` returned by §3's `pager(page, pages)`).

## Patterns

### 1. Option-light Command that opens a View

**When:** any interactive feature (UD-3). **Shape:** a unit `#[derive(SlashCommand)]`
struct (or one or two options), a pure `render_x(..) -> V2Message` in `view.rs`, and
`ctx.respond(render_x(..))`. Full code: `views-and-recipes.md` §1-2.

```rust
#[derive(SlashCommand, Debug)]
#[command("leaderboard", contexts = [Guild])]
pub struct Leaderboard;

impl Leaderboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let rows = load_rows(&ctx.kv(), ctx.guild_id()).await?;  // all I/O first
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_board(&rows, &s, 0)).await
    }
}
```

### 2. Refresh button: re-render in place

**When:** the data behind a message changes. **Shape:** a `#[derive(Button)]` whose
Payload holds what the View needs to reload (ids, page), and a handler that loads and
answers with `ctx.update(render_x(..))`. Full code: the template's `RefreshProfile`
(`views-and-recipes.md` §2).

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("board.refresh", defaults(style = Secondary, label = keys::COMMON_REFRESH))]
pub struct RefreshBoard {
    pub page: u16,
}

impl RefreshBoard {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let rows = load_rows(&ctx.kv(), ctx.guild_id()).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_board(&rows, &s, self.page)).await
    }
}
```

### 3. Pagination

**When:** a list longer than one message. **Shape:** a `page: u16` Payload field, one
Button struct for Previous and Next (each carries its target page, so the ids differ:
CID-02), disabled at the bounds, the page size from `noctis::ui::Budget`, the page clamped
because it is client input. Full code: `views-and-recipes.md` §3.2.

```rust
fn pager(page: u16, pages: u16) -> ActionRow {
    ActionRow::buttons([
        TurnPage { page: page.saturating_sub(1) }.build().label(keys::COMMON_PREVIOUS).disabled(page == 0),
        TurnPage { page: page.saturating_add(1) }.build().disabled(u32::from(page) + 1 >= u32::from(pages)),
    ])
}
```

### 4. Tabs

**When:** 2-5 views of one entity. **Shape:** a `#[derive(PayloadEnum)]` tab enum in the
Payload, one button per tab (the current one disabled or `Primary`), `match` on the tab in
the render function. Full code: `views-and-recipes.md` §3.1.

```rust
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab { Overview, Members, Settings }

#[derive(Button, Debug, Clone, Copy)]
#[button("dash.tab", defaults(style = Secondary, label = "Tab"))]
pub struct OpenTab { pub tab: Tab }
```

### 5. Step flow (wizard)

**When:** setup that takes several choices in order. **Shape:** a `step` PayloadEnum plus
the choices so far in the Payload (or a `by_ref` Route when they are large), Back/Next
buttons, one `render_step(..)`; save only on the last step. Full code:
`views-and-recipes.md` §3.3 and the template's `/setup start`.

### 6. Settings screen

**When:** re-editable configuration. **Shape:** a Section per setting with an accessory
button, selects pre-filled with the current value (a fresh nonce per send, so always set
the defaults), enable/disable toggles, `ctx.update(render_settings(..))` after each save.
Full code: `views-and-recipes.md` §3.4 and the template's `/dashboard`.

### 7. Button → modal → re-render

**When:** free text or several inputs. **Shape:** the button declares `auto_defer = false`
and takes `ComponentResponder<Manual>`; the modal's Payload carries what the submit needs;
the submit saves and answers with `ctx.update(..)`. Full code: `modals.md` §5 and the
template's `EditBio`/`BioForm`.

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("note.edit", auto_defer = false, defaults(style = Primary, label = keys::NOTE_EDIT))]
pub struct EditNote { pub id: u32 }

impl EditNote {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(NoteForm::blank(self.id)).await?.done())
    }
}
```

### 8. Rendered image in a Media Gallery

**When:** dense read-only data (cards, charts, leaderboards; UD-2). **Shape:** a
`#[derive(Template, Serialize, Deserialize)]` struct registered with
`Module::template::<X>()`, rendered in the handler with `ctx.render(x).await?`, placed in
a `MediaGallery` with alt text, buttons next to it. Full code: `images.md` §2 and §10, the
template's `ProfileCard`.

```rust
let image = ctx.render(BoardCard { rows: top10.clone() }).await?;
let mut files = AttachmentSet::new();
let card = files.add(image);
let gallery = MediaGallery::new([MediaItem::new(card).alt(s.board.alt())]);
V2Message::new(Components::new().push(Container::new().push(gallery).push(pager)))
    .attachments(files)
```

### 9. Confirmation for a destructive action

**When:** delete, reset, ban. **Shape:** a Danger button opens a confirm View whose
Confirm button is a `by_ref` Route with `one_shot` (the first click consumes the Ref) and
a short `ttl`. Full code: `routes-payloads-refs.md` §7.

```rust
#[derive(Button, Debug, Clone)]
#[button("board.reset", by_ref, one_shot, ttl = "5m", permissions = MANAGE_GUILD,
    defaults(style = Danger, label = keys::BOARD_RESET_CONFIRM))]
pub struct ConfirmReset { pub guild: u64 }
```

### 10. Owner-only controls

**When:** a message only its author may drive. **Shape:** an `Id<UserMarker>` Payload
field and `owner = field` on the kind attribute; the Guard answers everyone else with a
localized notice. Full code: `routes-payloads-refs.md` §1 (Route Guards).

### 11. Counters and scores in `Kv`

**When:** points, XP, usage counts. **Shape:** `ctx.kv().incr(&key, n, None)` (atomic) for
a counter, `ctx.kv().counter(&key)` to read it, a per-guild index (a `Vec<u64>` of user
ids under one key) when you need to list them. Keys are your own strings
(`points:<guild>:<user>`); Noctis prefixes them with the App id. Full code: `adapters.md`
§3.

```rust
let total = ctx.kv().incr(&format!("points:{guild}:{user}"), amount, None).await?;
let mut members: Vec<u64> = ctx.kv().get(&format!("points:{guild}:index")).await?.unwrap_or_default();
if !members.contains(&user.get()) {
    members.push(user.get());
    ctx.kv().put(&format!("points:{guild}:index"), &members, None).await?;
}
```

The index update is not atomic: a concurrent add can lose an id, and the snippet re-adds it
on that member's next `incr`.

### 12. Per-guild configuration with stored ids

**When:** a channel, role or webhook chosen once. **Shape:** a serde struct with `u64`
ids (never `Id<_>`), rebuilt with `Id::new_checked`. Full code: the template's
`GuildConfig` and `adapters.md` §3.

```rust
#[derive(Serialize, Deserialize, Default)]
struct GuildConfig { log_channel: u64 }

let channel = Id::<ChannelMarker>::new_checked(config.log_channel); // None = not set
```

### 13. Guild-only Middleware with an Extension

**When:** many handlers need the same check or loaded value. **Shape:**
`Module::before(Middleware::before(f).provides::<T>())`, handlers take `Ext<T>`. Full code:
`modules-middleware-ratelimits.md` §5 and the template's `guild_only`.

### 14. Ephemeral sub-panel on a shared message

**When:** per-user settings opened from a public message. **Shape:** a button with
`response = ephemeral` whose handler answers `ctx.respond(render_panel(..))`; the panel's
own buttons update the ephemeral message. Full code: `responder.md` §3.

## Anti-patterns

The short list is `SKILL.md` §5 (listeners and collectors, hand-built `custom_id`s, free
`async fn` handlers, per-call ephemerality, I/O in render functions, …). These come on top
of it:

| Never | Instead |
| --- | --- |
| Keeping UI state (page, tab, selection) in `Kv` keyed by message id | Typed Payload fields of the feature's Routes; `by_ref` when large |
| Storing `Id<_>` values (or structs holding them) with `ctx.kv().put` | `u64` (`id.get()`), rebuilt with `Id::new_checked` |
| `Id::new(n)` or `unwrap` on stored or cached data | `Id::new_checked(n)` and `Ok(None)` handling |
| A read-then-write counter (`get`, `+ 1`, `put`) | `ctx.kv().incr(..)` (atomic) |
| Hand-rolled cooldowns in handlers | `rate_limit(..)` on the kind attribute |
| A second HTTP client for Discord | `ctx.http()` (`ctx.http().raw(..)` as the last resort) |
| Hard-coded limits (`25`, `40`, `4000`) | `ctx.limits()` and `noctis::ui::Budget` |
| `std::thread::sleep` or blocking I/O in a handler | async I/O; defer is automatic |
| A `HashMap` in Template data | `BTreeMap` or `Vec` (render cache keys must be deterministic) |
| Formatting a Tailwind class at run time | `Style` setters (`width_pct`, `color_rgb`, …) |
| Sending your own error reply, then `Err` | `HandlerError::user(key)`: one notice, no sink |
| A `description = "…"` literal on a Command | `cmd.<path>.description` in `locales/en-US.yaml` |
| One Module per handler, or every feature in one Module | One Module per feature (`src/modules/<feature>/`) |

## Common mistakes

| Symptom | Cause | Fix | Rule |
| --- | --- | --- | --- |
| `PortError::Value(Malformed)` reading a value you stored with `ctx.kv()` | the value holds twilight `Id<_>`, which does not decode from postcard | store `u64` (`id.get()`) and rebuild with `Id::new_checked`; an already-stored value of the old shape: treat the error as absent and overwrite it | — |
| a test fails at start with an unexpected-argument error | `app.run()` parses the test harness's flags | `app.run_with_args(std::iter::empty::<&str>())`, or `TestApp` | — |
| E0599 `no method named update found for struct CommandCtx` | `ctx.update` in a Command (no message to update) | `ctx.respond(render_x(..))` | INT-04 |
| `[CMD-04] description differs from …` or a `description` argument error | a `description = "…"` literal on `#[command]` | delete it; write `cmd.<path>.description` in `locales/en-US.yaml` | CMD-04 |
| `[CMD-04] not found as cmd.<path>.description` | the key is missing in `locales/en-US.yaml` | add `cmd.<path>.description` (and each `cmd.<path>.opt.<name>.description`) | CMD-04 |
| a click answers the generic "not handled" notice; or ``[CID-04] Module `x` uses Route …`` | the component struct is not `.add::<X>()`ed to a mounted Module | `Module::new("x").add::<X>()` in exactly one Module; `uses::<X>()` elsewhere | CID-04 |
| a `[CID-04]` `BuildError` at `App::build` naming one Route twice | two Modules `.add` the same struct | keep one `.add`, use `uses::<X>()` in the other | CID-04 |
| `HandlerResult<()>` on a Command does not compile | a Command must answer | return `HandlerResult<Responded>` from `ctx.respond(..)`/`ctx.reply_text(..)` | INT-03 |
| a `[LOC-04]` warning at build | a key in `en-US.yaml` is missing in `tr.yaml` | add the key to `tr.yaml` with the same placeholders | LOC-04 |
| a compile error on a `keys::` path or a `Strings` method | the key is not in `locales/en-US.yaml`, or a placeholder name differs | add it to `en-US.yaml` first, then to every other locale | LOC-04 |
| `[INT-05] show_modal needs auto_defer = false on this handler` | a modal from an auto-deferred handler | `auto_defer = false` on the attribute and the `Manual` responder | INT-05 |
| "This interaction failed" after about 3 s | `auto_defer = false` on a handler that does not open a modal | remove `auto_defer = false` | INT-01 |
| `[OPT-01]` with ``= note: after optional `x` `` | a required field after an `Option<T>` field | move the `Option<T>` fields last | OPT-01 |
| a hand-written `custom_id` is never routed (generic notice) | `format!("x:{id}")` instead of a derive | a `#[derive(Button)]` struct and `.build()` | CID-03 |
| ``[CID-01] route `x` has unbounded fields`` | a `String`/`Vec` field in an inline Route | `by_ref` (or `overflow = spill`), or store the data and carry an id | CID-01 |
| `ResponderError::UpdateUnavailable` from a modal submit | the modal was opened by a slash Command, so there is no message to update | answer with `ctx.respond(..)` when `ctx.update(..)` reports `UpdateUnavailable` (the template's `BioForm`) | INT-04 |
| `[IMG-01]` at `App::build`, or text missing in a rendered image | no font registered | enable the `default-font` feature or add `ImageConfig::font(..)` | IMG-01 |
| a `[TW-01]` error at `cargo check` | a typo, a class Tailwind v4 does not have, or a theme token missing in `noctis-theme.css` | fix the class or add the `--<namespace>-<token>` to the theme | TW-01 |
| `[GW-05]` `MissingIntent` at `App::build` | a handler or cache needs a privileged intent | `.intents(Intents::GUILD_MEMBERS)` (or the one named) and enable it in the Developer Portal | GW-05 |
| more than 5 buttons in one row does not compile | `ActionRow::buttons` takes at most 5 | split into rows or use `ActionRow::try_buttons` | CV2-06 |
| a select shows an old value after a save | its defaults were not set for this render | always pass the current value as the select's defaults | CV2-09 |
| an ephemeral answer turns public (or the reverse) on an edit | ephemerality is fixed at creation | declare `response = ephemeral` on the attribute | INT-10 |
| a render returns `RenderError::QueueFull` | the render pool is busy | answer a "busy, try again" notice with your own key | — |
| `Id::new` panics on a stored `0` | `Id::new` rejects `0` | `Id::new_checked(n)` and treat `None` as no value | — |
| no way to find "the top 10" or every stored member in `ctx.kv()` | `Kv` cannot list or scan keys | keep an index (`Vec<u64>` of ids) under one key next to the `incr` counters (§11), or use your own database Adapter | — |
| ``locale file `locales/en-US.yaml` has errors (see `i18n!`): simple key expect ':'`` | YAML syntax: `key:"value"` without a space after the colon, or bad indentation | write `key: "value"` (a space after `:`), indent nested keys by two spaces, quote values that start with `#`, `{` or contain `: ` | — |
| `clippy -D warnings` fails with `item in documentation is missing backticks` | the template's clippy runs `pedantic`: identifiers in doc comments need backticks | write `` `render_board` ``, `` `ctx.kv()` `` in `///` and `//!` comments | — |
