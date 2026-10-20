---
name: noctis
description: >-
  The Noctis Usage Skill: how to build Discord bots with Noctis, the Rust framework on
  twilight-rs and tokio (built-in sharding, Components V2 UI kit, Tailwind image
  rendering). Use this skill WHENEVER you write, review, debug or explain code for a bot
  built on Noctis: slash commands and Command Patterns, options and autocomplete,
  context menus, Routes and Payloads, buttons and selects, Views, Navbar, Wizard,
  Paginator, modals, the Responder, Templates and images (tw!, view!), Adapters and
  ports, i18n and locale files, Modules, Middleware, rate limits, sharding and Clusters,
  publishing, and tests with the testkit. Also use it when a compile error or BuildError
  names a rule id like [CID-01] or [CMD-11], or when the user asks "how do I … in
  Noctis". It is the only allowed source of Noctis knowledge: never read Noctis source.
---

# Noctis Usage Skill

> **Status: v0.** This skill documents the **planned** Noctis API from the architecture
> spine. Sections and signatures marked **[planned]** may change as the crates land; the
> skill is updated in the same commit as any public API change.
> **API hash:** `8dbe851b7216b5514cce5845908473937223e19b106e7340de7b2e649d56568e` (blake3 of noctis-rs `xtask/public-api.txt`; updated in the same commit as every public API change, together with this skill).

This skill is the whole public API of `noctis`, explained for bot authors and coding
agents. **Learn Noctis only from here.** Do not read Noctis source (the monorepo crates,
`~/.cargo/registry/**/noctis*`, macro expansions). If something you need is missing,
say so plainly ("the Noctis skill does not cover X") instead of guessing or opening the
source.

For general Rust, use the `rust-best-practices` skill.

## 1. Mental model (read this first)

```text
Discord ──gateway──▶ Shard tasks ──▶ Interaction lane (never dropped) ─▶ pipeline ─▶ your handler
                                 └─▶ Event lane (bounded)               ─▶ pipeline ─▶ your #[event]
your handler ──returns──▶ View / StaticView / Responded ──▶ Responder ──▶ Discord HTTP
```

- **App** is the root: token, Modules, Adapters, state. `App::from_env()` → `&mut` builder →
  `.build()?` → `.run().await?`. Sharding is automatic. One binary also publishes
  commands (`--publish-guild <id>`).
- **Module** groups features (tescord's Pack): `Module::new("profile").add(...)`.
  Everything you register is a *descriptor* produced by a macro (`#[command]`,
  `#[handler]`, `#[event]`, `#[hook]`) and added with `.add(fn_name())`.
- **Command** = an `async fn` with a Command Path: `#[command("settings logs channel")]`.
  Commands are *entry points* that open Views (UD-3).
- **Route** = a typed struct with `#[derive(Route)]` and a stable id
  (`#[route("shop.buy")]`). Its fields are the **Payload**, encoded into the component's
  `custom_id`. Every button/select/modal is built *from a Route value*
  (`BuyItem { item, qty }.button()`), and each Route has exactly **one** `#[handler]`.
  Too big for 100 chars? `by_ref` stores it in the RefStore and puts an 11-char **Ref**
  in the id instead.
- **View** = a pure function from state to a Components V2 message:
  `impl View { fn render(&self, cx) -> UiResult<V2Message> }`. The same View renders the
  first response and every update. Handlers *return* Views; Noctis picks the response
  type, defers when slow, sets the V2 flag and validates every Discord limit.
- **Widgets** (`Navbar`, `Wizard`, `Paginator`) live inside a View and ride on the
  View's own Route (`#[route("id", view)]`): tab, page and step travel in the
  `custom_id`, so they need no handler code and no storage.
- **Handlers are stateless.** No collectors, no per-message closures, no timers. State
  lives in Payloads/Refs, in ports (`ctx.kv()` for your key-value data), or in
  `State<T>`. Cached Discord resources are read with `ctx.cache()`.
- **Adapters (ports)** are traits for anything that stores data, each with a bounded
  in-memory default; swap in Redis/SQL without touching handlers.
- **Templates** are Tailwind-styled images written in Rust (`view!`), rendered off the
  async runtime and attached to Views as Media Gallery or Thumbnail items.
- **Rules are checked early**: compile time for literals and types, `App::build` for
  cross-Module conflicts, runtime only for dynamic data. Every error names a rule id
  (`[CMD-11]`, `[CID-01]`, `[CV2-03]`) from `references/limits-cheatsheet.md`.

Everything is imported from one crate: `use noctis::prelude::*;` plus a few modules
(`noctis::rules`, `noctis::id`, `noctis::ui`, `noctis::image`, `noctis::ports`,
`noctis::events`, `noctis::testkit`). Never depend on `noctis-*` sub-crates.

## 2. UI Doctrine (normative rules)

Follow these in every bot. Reviews should flag violations.

| Rule | Do | Never |
| --- | --- | --- |
| **UD-1 V2 for anything interactive** | Build every interactive or dashboard surface as a Components V2 `View`. | Embeds or plain `content` as the primary output of an interactive feature. `LegacyMessage` is for plain notices only. |
| **UD-2 Images for read-only visuals** | Render dense read-only data (cards, charts, leaderboards) with a Template; place it in a Media Gallery (wide) or Thumbnail (small); always set alt text. | Put information the user must act on (buttons, links, instructions) inside an image. |
| **UD-3 Commands are entry points** | Option-less or option-light command → returns a View. Configuration and data entry happen inside the View (selects, modals, Wizard Steps). | Long option lists for configuration. Options are for quick actions, autocomplete lookups, context-menu targets. |
| **UD-4 Steps for setup flows** | Setup/config flows are a `Wizard` with "Step n of m: Title". | "Page 2 of 3" for a setup flow. |
| **UD-5 Navbar for general views that don't fit** | A general View that doesn't fit one Container splits into `Navbar` tabs (≤ 5). | Cramming unrelated sections into one scroll, or a 6th tab (typed error). |
| **UD-6 Paginate on overflow** | Homogeneous lists go through `Paginator` (default Overflow Policy); inside the active tab when there is a Navbar. | Hand-rolled page math, `take(25)`, or truncating a list to dodge a limit. |
| **UD-7 No listener per message** | One handler per Route; state in Payloads, Refs or ports. One-off confirmations use a Route with a `one_shot` Ref. | Collectors, await-component loops, closures stored per message, `tokio::spawn` timers per message. |
| **UD-8 Allocation-conscious** | Bounded caches, fixed-width Payload fields, pure `render`, I/O before rendering. | Unbounded maps keyed by guild/user/message; per-interaction global state. |

**UI quality rulebook** (details in `references/views-and-widgets.md`):
1. One primary action per View (one `ButtonStyle::Primary` or `Success`).
2. Visible active state (disabled active tab, `default` on the current select option).
3. Confirmation for destructive actions (Danger button → confirm View → one-shot Route).
4. Ephemeral for single-user UI (settings, wizards, errors); public for shared content.
   Locale follows: User Locale for ephemeral, Guild Locale for public.
5. A way back or cancel on every interactive View.
6. Designed empty states and error states (never a missing block or a raw error).

## 3. Golden-path recipes

Each recipe is complete and uses the same names as the project template.

### 3.1 An entry command that opens a View

```rust
use noctis::prelude::*;

#[command("dashboard", permissions = MANAGE_GUILD, contexts = [Guild])]
async fn dashboard(ctx: CommandCtx, meta: State<BotMeta>) -> HandlerResult<DashboardView> {
    let guild = ctx.guild_id().ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))?;
    DashboardView::load(&ctx, DashRoute { guild }, &meta).await
}

pub fn module() -> Module {
    Module::new("dashboard").add(dashboard()).add(open_dashboard())
}
```

No response code: returning a View sends it (FR-61). With the `i18n` feature the
description comes from `cmd.dashboard.description` in `locales/en-US.yaml`; without it, write
`description = "…"` in the attribute. A missing one is a compile error (CMD-04), and a
literal that differs from the locale file is a compile error too.

### 3.2 A Route, a button, its handler

```rust
#[derive(Route, Debug, Clone, Copy)]
#[route("dash.mod", permissions = MANAGE_GUILD)]   // id ≤ 24 chars [a-z0-9._-]; guard [planned]
pub struct ToggleModule { pub guild: Id<GuildMarker>, pub module: ModuleKey, pub enable: bool }

// in a View's render:
let toggle = ToggleModule { guild, module, enable: !enabled }
    .button()
    .label(s.common.enable())
    .style(ButtonStyle::Success);

#[handler]
async fn toggle_module(ctx: ComponentCtx, t: ToggleModule, meta: State<BotMeta>)
    -> HandlerResult<impl IntoResponse>
{
    ctx.modules().set_enabled(t.module.module_id(), ModuleScope::Guild(t.guild), t.enable).await?;
    Ok(DashboardView::load(&ctx, DashRoute { guild: t.guild }, &meta).await?.at(Nav::tab(2)))
}
```

Returning a View from a component handler updates the message in place.

### 3.3 A View with Navbar and auto-pagination

```rust
impl View for DashboardView {
    type Route = DashRoute;                       // #[route("dash", view)]
    fn state(&self) -> &DashRoute { &self.route }

    fn render(&self, cx: &mut ViewCx<'_, DashRoute>) -> UiResult<V2Message> {
        let s = Strings::of(cx.locale());
        let navbar = Navbar::new(cx, [s.dashboard.tab.overview(), s.dashboard.tab.roles()])?;
        let body = match navbar.active() {
            0 => Body::from(vec![TextDisplay::new(s.dashboard.overview(/* … */)).into()]),
            _ => Paginator::new(&self.roles, |r| TextDisplay::new(r.mention.clone()))
                .empty(TextDisplay::new(s.dashboard.no_roles()))
                .into(),
        };
        Layout::new()
            .accent(rgb!(0x5865_F2))
            .header(TextDisplay::new(format!("## {}", s.dashboard.title(&self.guild_name))))
            .navbar(navbar)
            .body(body)
            .footer(ActionRow::buttons([cx.reload_button().label(s.common.refresh())]))
            .build(cx)
    }
}
```

The View Route needs a loader handler (`#[handler] async fn open_dashboard(ctx:
ComponentCtx, r: DashRoute, …) -> HandlerResult<DashboardView>`); core calls it for
tab, page and jump clicks after moving `ctx.nav()`.

### 3.4 A Wizard

```rust
let wizard = Wizard::new(cx, [s.setup.step.channel(), s.setup.step.roles(), s.setup.step.confirm()])
    .require(0, st.log_channel.is_some(), s.setup.need_channel())
    .require(1, st.mod_role.is_some(), s.setup.need_role());
let body = match wizard.step() { 0 => step_channel(..), 1 => step_roles(..), _ => summary(..) };
Layout::new().wizard(wizard.finish(FinishSetup { .. }.button().label(s.setup.finish())))
    .body(body)
    .build(cx)
```

Back/Next/Cancel are automatic. Selected values travel in the Payloads of the select
Routes (`PickLogChannel { owner, mod_role }`), never in a listener.

### 3.5 A rendered image in a View

```rust
#[derive(Template, Serialize, Deserialize)]   // the Renderer decodes the value it gets
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
pub struct ProfileCard { pub name: String, pub progress: f32, #[asset] pub avatar: RemoteImage }

impl TemplateView for ProfileCard {
    fn view(&self, _frame: Frame) -> Node {     // Frame::STILL unless animated
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                <img src={&self.avatar} tw="w-40 h-40 rounded-full" />
                <span tw="text-5xl font-bold text-white">{&self.name}</span>
            </div>
        }
    }
}

// in a handler (rendering is awaited in the handler, so auto-defer covers it):
let card: Attachment = ctx.render(ProfileCard { .. }).await?;
// in render():
MediaGallery::new([MediaItem::new(cx.attach(&self.card)).alt(s.profile.card_alt(&name))])
```

### 3.6 A modal with pre-fill

```rust
#[derive(Route, Clone, Copy)] #[route("profile.bio", opens_modal, owner = user)] // runtime args [planned]
pub struct EditBio { pub user: Id<UserMarker> }
#[derive(Route, Clone, Copy)] #[route("profile.save", owner = user)]
pub struct SaveBio { pub user: Id<UserMarker> }

#[derive(Modal, Debug)]
#[modal(route = SaveBio, title = "Edit your profile")]   // literal texts, checked at cargo check
pub struct BioForm {
    #[text_input("Bio", style = paragraph, max_length = 300)]
    pub bio: Option<String>,                              // Option ⇒ not required
}

#[handler]
async fn edit_bio(ctx: ComponentCtx, e: EditBio, r: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    let modal = BioForm::modal(SaveBio { user: e.user }).bio(current_bio); // pre-fill [planned, Story 9.4]
    Ok(r.modal(modal).await?.done())
}

#[handler]
async fn save_bio(ctx: ModalSubmitCtx, s: SaveBio, form: BioForm) -> HandlerResult<ProfileView> { /* … */ }
```

### 3.7 Wiring the App

```rust
pub fn configure(b: &mut AppBuilder) -> &mut AppBuilder {
    b.app_id("noctis-template")
        .locales(strings::catalog())
        .theme(noctis::image::theme!())
        .cache_store(adapters::BoundedCacheStore::new(50_000))
        .state(BotMeta { /* … */ })
        .module(modules::setup::module())
        .module(modules::dashboard::module())
        .module(modules::profile::module())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    configure(&mut App::from_env()?).build()?.run().await?;
    Ok(())
}
```

## 4. Decision table: which surface?

| Situation | Use | Why |
| --- | --- | --- |
| Configure something in ordered steps (setup, onboarding, create-X) | **Wizard** | UD-4: progress "Step n of m", validation per Step, Back/Cancel |
| Re-editable settings of one entity | **View** with Sections (value + "Edit" accessory) | Faster than a Wizard for edits; schwi-style settings panel |
| A general View with distinct sections that don't fit one Container | **Navbar** (≤ 5 tabs) | UD-5 |
| A homogeneous list that can exceed 40 components / 4000 chars | **Paginator** (inside the active tab) | UD-6; never hand-roll paging |
| Pick one of many entities (> 5) | **Paginator** `.as_select(..)` or a String Select | 25 options cost 2 components |
| Dense read-only data: cards, charts, tables, leaderboards | **Template image** in Media Gallery / Thumbnail | UD-2; 1 component, 0 text chars |
| Free text, numbers, or 2+ inputs that must be submitted together | **Modal** opened from a button | Message selects fire per change; modals batch |
| Destructive action | Danger button → confirm View → `one_shot` by-ref Route | Quality rule 3 |
| Per-user settings on a shared message | Ephemeral sub-panel (`#[handler(response = ephemeral)]`) | INT-10: ephemerality is fixed at creation |
| A one-line notice, no interaction | `HandlerError::user(key)` or a small `StaticView` | Localized, ephemeral |
| Post to a log channel or white-label persona | `StaticView` through a webhook | WH-01/02 |

Image vs text: if users need to **read** many numbers at a glance → image; if they need
to **act** on an item → a Section with an accessory button or a select option.

## 5. Anti-patterns (and the fix)

| Never | Instead |
| --- | --- |
| A per-message listener, collector, `await` for the next click, or `tokio::spawn` + sleep tied to a message | One `#[handler]` per Route; state in the Payload/Ref; expiry via Ref TTL and the Expired Handler |
| Hand-building a `custom_id` string (`format!("buy:{id}")`) | `#[derive(Route)]` struct → `.button()` / `.string_select(..)`; Noctis owns the format (AD-5) |
| Free text (reasons, names, URLs) in a Payload | Bounded ASCII `BoundedStr<N>` only if tiny; otherwise `by_ref`, or store it via a port and put the id in the Payload |
| `String`/`Vec` fields in an inline Route | They are unbounded → compile error `[CID-01]`; use `by_ref` / `overflow = spill` |
| Trusting Payload data for authorization | Route Guards (`owner = field`, `permissions = …`) plus checks in the handler; Payloads are client input (CID-06) |
| Choosing response types, calling defer/edit by hand when a View would do | Return the View; use the Responder only for modals, follow-ups, or multi-step replies |
| `content`/embeds for an interactive feature | A V2 View (UD-1) |
| Doing I/O in `View::render` | Load data in the handler (or a `load` fn), keep `render` pure |
| Formatting Tailwind classes at runtime (`format!("w-[{}%]", p)`) | Typed style setters: `style={Style::new().width_pct(p)}` |
| Putting buttons, instructions or links in an image | Components next to the image (UD-2) |
| Hard-coded English in Views or Templates | Locale keys via `Strings::of(cx.locale())` |
| `static mut`, global `HashMap`s, `lazy_static` registries | `State<T>` or a port |
| More than 5 tabs, more than 5 buttons in a row, two selects in a row | Typed errors / compile errors; restructure (select nav, more rows, a Paginator) |
| Publishing on every start or on shard ready | `--publish-guild` in dev, `--publish` (or one `NOCTIS_PUBLISHER` process) in prod; publish is diff-based |
| Reading Noctis source to learn the API | This skill; say what is missing |

## 6. tescord → Noctis concept map

| tescord / dbi | Noctis |
| --- | --- |
| `Tescord` root, `Pack` | `App`, `Module` (`Module::new("a/b")`, nested with `.module(..)`) |
| `pack.slashCommand({ name: "a (b\|c)" })` | `#[command("a (b\|c)")]`; the handler gets a generated `APath` enum |
| `options: { user: { type: "User" } }` | `#[derive(Options)] struct … { user: ResolvedUser }` |
| `autoComplete` | `#[option(autocomplete = fn)]` + an `#[autocomplete]` fn returning `Ok(ctx.respond(choices))` (sync, no Module registration; `commands.md` §4) |
| `pack.button({ id, options })` + `buildComponent({ id, data, overrides })` | `#[derive(Route)]` + `Route { .. }.button().label(..)` + `#[handler]` |
| `data[i] as number` | typed Payload fields |
| dbi references / store | `by_ref` Routes + `RefStore` |
| `locale.user.x.y(a)` / `locale.guild…` | `Strings::of(ctx.user_locale()).x.y(a)` / `ctx.guild_locale()` |
| `*.interactions.yaml` `names:` | `cmdword.<word>` and `cmd.<path>.*` keys in the same locale files |
| `beforeInteraction` returning `false`/object | Middleware returning `Flow::Stop` / inserting an `Ext<T>` |
| `--publish [--guildId]` | `--publish`, `--publish-guild <id>`, `--dry-run` |
| `--features a,b` | Boot Flags: `Module::flags([..])` + `--features a,b` |
| inline listeners | not supported (UD-7): Route + `one_shot` Ref |

## 7. Reference index (progressive disclosure)

Read only what the task touches.

| File | Read when you… |
| --- | --- |
| `references/app-and-sharding.md` | build/run the App, single-binary assets and the first-run `.env`, env vars, CLI modes, sharding, Clusters, HTTP proxy, presence, lifecycle hooks, shutdown, plugins (`.image`, `.theme`, `PrefixPlugin`), multiple Apps and `Runtime` |
| `references/commands.md` | declare commands, Command Patterns, options, choices, autocomplete, context menus, metadata, Publish |
| `references/routes-payloads-refs.md` | declare Routes, Payload field types, the `custom_id` format and length proof, `by_ref`/spill, TTL, one-shot, guards, stale/expired handling, schema evolution |
| `references/responder.md` | need explicit replies, defers, follow-ups, modals, ephemerality, auto-defer, errors shown to users, webhooks |
| `references/components-v2.md` | **where each component can be used (placement matrix: messages, Containers, Sections, Action Rows, modals, Labels, legacy messages)**; build any component: every type, where it may go, every field and limit, builder sketch, `components!` literal trees, `label!`/`rgb!` |
| `references/views-and-widgets.md` | write a View, Layout, Navbar, Wizard, Paginator, attachments, mentions; UI quality rulebook and patterns |
| `references/modals.md` | build modals, Labels, Text Input, selects, File Upload, Radio/Checkbox, pre-fill, submissions |
| `references/images.md` | write Templates, `view!`/`tw!`, Tailwind v4 names, theme, fonts, emoji, remote images, animation, render pool, cache |
| `references/adapters.md` | the in-memory defaults (`noctis::ports::memory`: limits, usage, `MemoryDiscordCache`), swap or implement a port (RefStore, CacheStore, DiscordCache, …), conformance tests, namespacing, `ctx.kv()` and `ctx.cache()` |
| `references/i18n.md` | locale files, typed strings, command localization, locale resolution |
| `references/modules-middleware-ratelimits.md` | Modules, Boot Flags, enable/disable, Middleware and Extensions, Error Sinks, rate limits, events |
| `references/testing.md` | test commands, clicks, Views, snapshots, Templates, Adapters, memory |
| `references/limits-cheatsheet.md` | look up any numeric limit or rule id |
| `references/troubleshooting.md` | decode a compile error, BuildError or runtime error, or debug "interaction failed" |
