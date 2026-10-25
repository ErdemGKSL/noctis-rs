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
- **Stories 6.10 and 6.11** close Epic 6. Story 6.10 removes the widget code that
  Epics 1–6 already shipped and converges the Command Responder (§2); Story 6.11
  converts the shipped authoring surface to one struct per interaction (§7). Epic 7
  onwards assumes both.

## 2. Interaction responses follow discord.js exactly

These semantics were verified against discord.js 14.27.0 `InteractionResponses`.
Each method is a distinct flow:

| Method | Callback / route | Effect |
| --- | --- | --- |
| `reply(msg)` / `reply_as(msg, eph)` | type 4 | new message; ephemerality is fixed here |
| `defer_reply()` / `defer_reply_as(eph)` | type 5 | new "thinking…" message |
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
  - Handlers declared `auto_defer = false` have no auto-defer, so they must open the
    modal within 3 s.
  - The typestate removes the method after an ack.
  - A runtime race returns `ModalUnavailable` and sends no request.
- **Per-handler `auto_defer` (replaces `opens_modal`).** Every interaction handler has
  an `auto_defer` flag, default `true`, declared where the handler's other
  registration arguments live (§7): `#[command(.., auto_defer = false)]`,
  `#[user_command(..)]`, `#[message_command(..)]`, and the kind attribute of a
  component or modal (`#[button(..)]`, `#[string_select(..)]`, `#[modal(..)]`, …). Autocomplete has no defer,
  so `auto_defer` on `#[autocomplete]` is a compile error (INT-06). The deadline
  itself stays App-wide (`AppBuilder::ack_deadline`, 2000 ms default).
  - `opens_modal` and the `OpensModal`/`NoModal` markers are removed everywhere (Story
    6.10 converts the shipped ones); `auto_defer = false` replaces them.
  - The responder carries the choice in its type: `Responder<K, S = Pending, D =
    AutoDefer>` with the sealed `D = AutoDefer | Manual`, through the aliases
    `CommandResponder`, `ComponentResponder`, `ModalResponder` (all `AutoDefer` by
    default) and `CommandResponder<Manual>`, `ComponentResponder<Manual>`,
    `ModalResponder<Manual>`.
  - With `auto_defer = true` (the default) the responder has **no** `show_modal`:
    calling it is a compile error whose message says `[INT-05] show_modal needs
    auto_defer = false on this handler`. The `Manual` alias is required exactly when
    `auto_defer = false` is declared, and vice versa (a mismatch is an INT-05 compile
    error, checked between the derive's declared mode and the `#[handler]` method's
    responder parameter).
  - With `auto_defer = false` no auto-defer is armed, `show_modal` is available for
    commands and components (never for modal submits), the handler may still call
    `defer_reply`/`defer_update` itself, and missing the 3 s deadline logs an error
    naming the handler.
  - A button that opens a modal must be `auto_defer = false`.
- **Declared `response` (one knob, compile-time checked).** Every handler declares at
  registration what its (auto-)defer and default response look like, with one
  attribute argument `response = …` (no separate `ephemeral` flag):
  - commands and context menus: `response = public` (default) or `ephemeral`: the
    auto-defer is type 5 without or with `EPHEMERAL`, and `reply`/`defer_reply` use
    that ephemerality by default; `update` is a compile error (INT-04);
  - components and component-opened modal submits: `response = update` (default,
    auto-defer type 6, `update` in place), `ephemeral` or `public` (auto-defer type 5
    without or with `EPHEMERAL`, a new message);
  - slash-opened modal submits can only be `public` or `ephemeral`. Which interaction
    opened a modal is not known at compile time, so `response = update` on a modal
    submit is legal; when the submit has no source message, the auto-defer falls back
    to type 5 public and `update`/`defer_update` return `UpdateUnavailable` without a
    request. The ergonomic `ctx.update(msg)` behaves the same: it returns
    `UpdateUnavailable`, sends nothing and never falls back to a new message; the
    author answers such a submit with `ctx.respond(msg)`.
  - `response` is independent of `auto_defer`: with `auto_defer = false` it still sets
    the default ephemerality of `reply(msg)` and `defer_reply()`; `reply_as(msg, e)`
    and `defer_reply_as(e)` override it per call. Modal submits declare it on their
    `#[modal(..)]`.
  - Examples: `#[command("settings", response = ephemeral)]`, `#[button("mod.ban",
    response = ephemeral, auto_defer = true, defaults(..))] struct Ban { .. }`,
    `#[button("profile.bio", auto_defer = false, defaults(..))] struct OpenBioModal {
    .. }` (the `#[handler]` method itself takes no arguments, §7).
  - Story 6.10 converges the shipped bare `ephemeral` flag of `#[command]` and the
    shipped `defer(Ephemerality)` method onto `response = ephemeral` and
    `defer_reply()`/`defer_reply_as(e)`.
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

## 7. One struct per interaction: kind derive, mandatory defaults, handler on `self`

One struct = its id + its typed data + its handler, like tescord's `pack.button` /
`pack.stringSelect` registrations. Free-function handlers are not part of the API;
there is one style only.

- **The derive names the kind.** Components: `#[derive(Button)]`,
  `#[derive(StringSelectMenu)]`, `#[derive(UserSelectMenu)]`,
  `#[derive(RoleSelectMenu)]`, `#[derive(MentionableSelectMenu)]`,
  `#[derive(ChannelSelectMenu)]`, `#[derive(Modal)]`. Commands:
  `#[derive(SlashCommand)]`, `#[derive(UserCommand)]`, `#[derive(MessageCommand)]`,
  whose struct fields are the typed options (this replaces `#[derive(Options)]` plus
  a function attribute). Gateway events use `#[derive(EventHandler)]` on a unit
  struct, so every registration has one shape. Link and Premium buttons have no
  handler and stay plain builders (`LinkButton::new(url)`).
- **One attribute per kind**: the id first, then the routing arguments, then (for
  components and modals) a **mandatory** `defaults(..)` block:

  ```rust
  #[derive(Button)]
  #[button("punish-history-set", response = update,
           defaults(style = Secondary, label = "punish.next", emoji = "➡️", disabled = false))]
  struct PunishHistorySet { user: Id<UserMarker>, guild: Id<GuildMarker>, page: u16 }

  impl PunishHistorySet {
      #[handler]
      async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> { /* … */ }
  }

  #[derive(StringSelectMenu)]
  #[string_select("ticket-system", defaults(placeholder = "ticket.pick", min_values = 1, max_values = 1))]
  struct TicketSystem;

  #[derive(Modal)]
  #[modal("bio", defaults(title = "profile.bio_title"))]
  struct BioForm {
      user: Id<UserMarker>,                                   // Payload field (in the custom_id)
      #[text_input("Bio", style = paragraph, max_length = 300)]
      bio: Option<String>,                                    // input field (submitted, pre-filled)
  }

  #[derive(SlashCommand)]
  #[command("ban user", description = "Ban a member", response = ephemeral)]
  struct Ban { #[option(description = "Who")] user: ResolvedUser, #[option(description = "Why")] reason: Option<String> }
  impl Ban { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { /* … */ } }
  ```

  Routing arguments: `response`, `auto_defer` (§2), the codec arguments (`by_ref`,
  `overflow = spill`, `schema = n`, `signed`; a select kind always carries the select
  nonce, so the bare `select` argument is gone) and the runtime arguments (`ttl`,
  `one_shot`, `owner`, `permissions`, `guard = path`, `rate_limit(..)`). Expired and
  stale handling are methods too: `#[on_expired]` / `#[on_stale]` in the same `impl`
  (no `self`, since the Payload did not decode). Commands keep their metadata arguments (`description`,
  `permissions`, `contexts`, …) and have no `defaults` block. A command without
  options is a unit struct (`struct Dashboard;`); a Pattern generates `<Struct>Path`,
  taken as the handler's parameter after `ctx`.
- **The handler is `impl X { #[handler] async fn handle(self, ctx: …) }` for every
  kind**; `#[handler]` takes no arguments. `self` is the decoded Payload (component),
  the decoded options (command), the Payload plus submitted values (modal), or the
  unit struct (context menu, which also takes its resolved target; event, which
  also takes the event payload). Autocomplete handlers are methods on the command
  struct: `#[autocomplete(field)] async fn …(ctx: AutocompleteCtx, typed: &str, filled:
  Partial<Self>) -> HandlerResult<Suggestions<T>>`, which take no `self`, `response` or
  `auto_defer` (§8).
- **Modals.** Fields without an input attribute are the Payload (encoded in the
  `custom_id`, length-proven like a Route); fields with `#[text_input]`,
  `#[radio_group]`, … are the inputs. Building from a struct value pre-fills the inputs
  with its values (`None` = empty); the derive also generates `X::blank(<payload
  fields>)`, which builds with empty inputs for forms whose required inputs have no
  empty value. The shipped `#[modal(route = R, title = "…")]`
  form and its separate submit Route converge on this (Story 6.11).
- **`defaults(..)` is mandatory.** A missing block, or a missing required field for
  the kind (Button: `style` and `label` or `emoji`; selects: `placeholder`, plus
  optional `channel_types` on channel selects and `options = ChoicesEnum` on string
  selects; modal: `title`) is a compile error naming the kind's required fields; unknown keys are
  compile errors; literal values are checked against Discord's limits at compile time
  (CV2-08, CV2-09, MOD-01, …). Values the kind fixes when omitted are documented:
  `min_values = 1`, `max_values = 1`, `disabled = false`. Text in `defaults` follows
  the `description` rule (AD-16): a literal without the `i18n` feature; with it, a
  locale key resolved per response locale and checked against the default-locale
  file at compile time.
- **`.build()`** emits the component from the defaults plus the struct's Payload as
  the `custom_id` (tescord `buildComponent({ id, data, overrides })`). Overrides are
  chainable typed setters that replace one field each: `.label(..)`, `.emoji(..)`,
  `.style(..)`, `.disabled(..)`, `.placeholder(..)`, `.options(..)`,
  `.default_values(..)`, `.min_values(..)`, `.max_values(..)`; for modals `.title(..)`
  and `.value(field, ..)`. Anything not overridden comes from the defaults; overrides
  are validated at run time (Strict or Lenient). A string select's options come from
  `defaults(options = SomeChoicesEnum)` or per render; with neither, `.build()`
  returns a builder whose typestate requires `.options(..)` before it converts into
  a component (a compile error otherwise).
- **Registration** is by type: `Module::add::<T>()` (`module.add::<PunishHistorySet>()`,
  `module.add::<Ban>()`), replacing the shipped `Module::add(descriptor)`.
- **CID-04 stays**: no `#[handler]` impl, no `.build()`; one id, one handler; the
  `custom_id` length proof (CID-01) is unchanged.
- **Conversion.** Story 6.11 converts the shipped code: free-function `#[command]`,
  `#[user_command]`, `#[message_command]` and `#[event]` handlers, `#[derive(Options)]`,
  `#[derive(Route)]`/`#[route]` and the `.button()`/`.string_select(..)`/… constructors,
  the `#[modal(route = ..)]` form, and `Module::add(descriptor)`.

## 8. Autocomplete: a flagged option paired with a suggestion method

The option carries a bare `autocomplete` flag; the suggestions come from a method on the
same command struct, named by `#[autocomplete(field)]`. Noctis sends the response.

Commands have no `defaults(..)` block: that block exists only for components and modals,
whose rendered fields can be overridden per render. Command metadata is fixed at
registration, so it is written directly: `#[command("warn", description = "warn.desc", ..)]`
(a locale key with the `i18n` feature, a literal otherwise; §7).

```rust
#[derive(SlashCommand)]
#[command("warn", description = "warn.desc", response = ephemeral)]
struct Warn {
    #[option(description = "warn.user")]
    user: ResolvedUser,
    #[option(description = "warn.rule", autocomplete)]
    rule: i64,
}
impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> { /* … */ }

    #[autocomplete(rule)]
    async fn rule(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<i64>> {
        let rules = ctx.state::<Db>().search_rules(ctx.guild_id()?, typed, 25).await?;
        Ok(rules.into_iter().map(|r| (r.id, r.name)).collect())
    }
}
```

- **Flag and method pair up, checked at compile time.** Each of these is a compile error:
  - an `autocomplete` flag without an `#[autocomplete(field)]` method (OPT-11);
  - an `#[autocomplete(field)]` method whose field has no `autocomplete` flag, or names
    a field that does not exist (OPT-11);
  - `autocomplete` on a `Choices`-typed field (OPT-10);
  - `autocomplete` on a type other than STRING, INTEGER or NUMBER (OPT-11).
  The shipped `autocomplete = path` argument is gone; `autocomplete` is a bare flag.
- **Parameters.** The method has no `self` (the options are incomplete) and takes
  exactly `(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>)`.
  - `typed` is the raw focused input. Discord sends it as a string even for INTEGER and
    NUMBER options, and it may be partial (`"1"`, `"-"`, `""`); parsing it is the
    method's job.
  - `filled` holds every other option as `Option<_>`. It is untrusted and partially
    filled (OPT-12); the command handler re-validates the final values.
- **Return type.** `HandlerResult<Suggestions<T>>`, where `T` is the field's type with
  `Option` removed (`i64`, `f64` or `String`); another `T` is a compile error.
  `Suggestions<T>` collects from `(T, label)` pairs whose label is `String`, `&str` or
  `MessageKey` (resolved in the user's locale).
- **Noctis sends the response** (callback type 8). `AutocompleteCtx` has no `respond`
  and no Responder.
  - More than 25 suggestions: the first 25 are sent and a warning is logged, in every
    mode (INT-07).
  - A label over 100 characters is truncated with `…` and a warning, in every mode.
  - A suggestion Discord would reject in another way (a `String` value over 100
    characters, an INTEGER outside ±2^53, a non-finite NUMBER) is dropped with a
    warning, because changing a value would change its meaning.
  - An error, a panic or a missed deadline sends an empty list and routes the error to
    the Autocomplete Error Sink.
- **No defer, 3 s budget.** Autocomplete cannot be deferred: `auto_defer` or `response`
  on `#[autocomplete]` is a compile error (INT-06). The method must finish within 3 s;
  the response goes through the Discord edge (Story 5.9, §3).
- **Context helpers.** On `AutocompleteCtx`, `guild_id()` returns
  `Result<Id<GuildMarker>, HandlerError>` (an error outside a guild, which answers an
  empty list), and `state::<T>()` returns `State<T>` (a `T` never registered with
  `AppBuilder::state` panics with the type's name; the panic is caught like any
  handler panic).
- **Registration.** `module.add::<Warn>()` registers the command and its autocomplete
  methods together.
- **Conversion.** Story 6.7 ships the runtime (Suggestions, truncation, empty list on
  failure, the 3 s budget). Story 6.11 converts whatever 6.7 shipped on the old surface
  (`autocomplete = path`, a free function, `ctx.respond(choices)`) to this form.

## 9. Modal pre-fill is part of Route defaults and overrides (2026-10-22)

- Modal pre-fill (FR-63) is the modal case of Story 9.3's per-render overrides: `.build()`
  pre-fills the inputs from the struct value, `.value(field, ..)` and `.title(..)` override
  one render. Story 9.4's acceptance criteria moved into Story 9.3, and 9.4 is not
  implemented as a separate story.
- Overrides are flexible: every kind gets an overrides struct (`ButtonOverrides`,
  `StringSelectOverrides`, …, and per modal `<Modal>Overrides`) with every field an `Option` and a
  `Default` impl, applied with `.overrides(ButtonOverrides { label: Some(..),
  ..Default::default() })` next to the chainable setters (the later call wins). Modal
  pre-fill uses struct update syntax over the blank value: `BioForm { bio: Some(v),
  ..BioForm::blank(user) }`. The Usage Skill shows both forms with examples.
- A modal struct is its submitted data, not its component layout: the field attributes
  declare the components, the field values are the Payload plus the input values (what
  the submit decodes into and what pre-fill sends). A modal's overrides struct is
  generated per modal (`BioFormOverrides { title, bio: InputOverrides, .. }`, field names
  checked by rustc) and changes presentation only (title; per input label, description,
  placeholder), never values.
- Literal versus localized text is decided by syntax, never by the `i18n` feature (this
  replaces §7's "a literal without the `i18n` feature; with it, a locale key"): a string
  literal is always hard-coded text, a path (`keys::PROFILE_SAVE`, a `MessageKey`) is
  always a locale key. At run time overrides take `impl Into<Text>` (`&str`/`String` or
  `MessageKey`). Existing key-looking literals in the template (`label =
  "profile.edit_bio"`) become `keys::…` paths when Story 11.1 ships.

## 10. Voice and music bots through Songbird (2026-10-25)

The maintainer asked for Songbird integration with an optimized Opus path so music
bots can be built on Noctis. This reverses the PRD non-goal "no voice or music
support" (Lavalink stays out); it becomes FR-80, AD-30 and Epic 17.

- **Opt-in crate.** Voice lives in a new layer-5 crate `noctis-voice`, reached through
  the facade feature `voice` (off by default) and core's `Plugin` seam (AD-24):
  `VoicePlugin` plus the extension trait `AppBuilderVoiceExt::voice(VoiceConfig)`.
  Nothing in core, gateway or the facade's default build names Songbird.
- **Songbird is the voice driver, not the gateway glue.** `songbird` 0.6.1 (DAVE E2EE
  through `davey`, RTP, UDP, mixer, symphonia inputs) is used with
  `default-features = false` and only its `driver` feature plus one websocket/TLS
  feature that keeps AD-21's ring-only rustls graph. Its `twilight`, `serenity` and
  `gateway` features and its `Songbird` manager are not used: songbird 0.6.1 caps
  twilight at `<=0.17`, which would pin Noctis's twilight upgrades to Songbird's
  release cycle, and its manager sends op 4 outside Noctis's per-shard send budget.
  Instead `noctis-voice` owns one `songbird::Call` per guild in voice (a map bounded by
  the guilds with a live call), built with `Shard::Generic`, whose `VoiceUpdate` sends
  op 4 Voice State Update through `noctis-gateway`'s command path (GW-04 budget, owning
  shard, Shard Range checks). The bot's own `VOICE_STATE_UPDATE` and every
  `VOICE_SERVER_UPDATE` are fed to the right `Call` with `update_state` /
  `update_server`.
- **Codec (maintainer, 2026-10-25): symphonia for demux and decode, libopus for
  encode.** Inputs are demuxed and decoded by symphonia (the 0.5 line Songbird depends
  on; no second symphonia version in the graph). Opus encoding uses Songbird's libopus
  binding (`opus2`, statically built from source, so the `voice` feature needs a C
  compiler and cmake, documented like the `image` feature's libwebp). Pre-encoded
  Opus (Ogg Opus, WebM/Matroska Opus at 48 kHz) is played by passthrough: packets go to
  the wire without decode or re-encode whenever only one track plays at unity volume,
  which is the common music-bot case. Decode plus mix plus encode is the fallback for
  other formats, several tracks or volume changes.
- Amended (2026-10-25, Story 17.1): Songbird features are `driver`, `gateway` (only
  for `Call`/`Shard::Generic`; its `Songbird` manager is never used), `rustls`, `tws`.
  `gateway` adds no twilight or serenity and no send path, so both reasons above
  still hold.
- **Non-Discord HTTP.** Music inputs fetch arbitrary media URLs, an exception to
  AD-24's "non-Discord HTTP only in `AssetFetcher`": `noctis-voice` may fetch media
  through Songbird's input HTTP client with a connect timeout and redirect cap; it
  never sends the bot token or Noctis headers and never reaches Discord's API.
- **Order.** Epic 17 runs on its own story-loop lane after Story 16.1, concurrently
  with Epic 16; merges serialize through the merge lock. Epic 14's template, skill
  and docs stories cover the `voice` feature once Epic 17 is merged.
