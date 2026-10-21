---
title: 'Story 6.11: Simplification: one struct per interaction'
type: 'refactor'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modals.md'
warnings: ['oversized', 'multiple-goals']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Epics 2-6 shipped four authoring styles: free-function `#[command]`/`#[user_command]`/`#[message_command]`/`#[event]`/`#[autocomplete]` handlers, `#[derive(Options)]`, `#[derive(Route)]` + `#[route]` with the `RouteComponents` constructors, and `#[derive(Modal)] #[modal(route = R, title = ..)]` with a separate submit Route. All are registered by `Module::add(descriptor)`. Maintainer decisions §7 and §8 (binding) allow exactly one style. Epic 7 builds component dispatch on that style. Implements the correct course of 2026-10-20, FR-59 (mandatory defaults), FR-76; AD-6, AD-9, AD-17, AD-19, AD-23, AD-26; rules CID-01..04, CID-07, CV2-08, CV2-09, MOD-01, CMD-11, INT-05, INT-06, OPT-10..12.

**Approach:** Every interaction becomes one struct:
- a kind derive names the kind: `SlashCommand`, `UserCommand`, `MessageCommand`, `EventHandler`, `Button`, the five `…SelectMenu` derives, or `Modal`;
- one kind attribute carries the id, the routing/metadata arguments and, for components and modals, a mandatory `defaults(..)` block;
- the handler is `impl X { #[handler] async fn handle(self, ctx, ..) }`;
- autocomplete is `#[autocomplete(field)]` methods returning `Suggestions<T>`;
- components are built with `X { .. }.build()`;
- registration is `Module::add::<X>()`.

The old forms fail to compile with a help showing the new form. Component and modal **dispatch**, their responders and their runtime arguments stay in Epic 7 (Stories 7.1, 7.2, 7.4). Story 9.3 adds the remaining overrides and the i18n defaults. Story 9.4 adds modal pre-fill.

## Boundaries & Constraints

**Always:**

- **Commands.**
  - The struct's named fields are the options, with the existing `#[option(..)]` arguments, rules and validators (Story 6.2). A command without options is a unit struct. `#[derive(SlashCommand)]` implements `Options` and generates `<Struct>Partial`.
  - `#[command("path or pattern", ..)]` takes the 6.1/6.3/6.10 arguments unchanged: `description`, `max_expansions`, `response`, `auto_defer`, metadata. A Pattern with a group generates `<Struct>Path` (same variant rules as the shipped `<Fn>Path`).
  - Handler shapes:
    - commands: `(self, ctx: CommandCtx[, path: <Struct>Path][, r: CommandResponder<D>]) -> HandlerResult<R: IntoResponse<Command>>`;
    - context menus: `#[derive(UserCommand)] #[user_command("Name", ..)]` / `#[derive(MessageCommand)] #[message_command("Name", ..)]` on unit structs, handler `(self, ctx: CommandCtx, target: ResolvedUser | ResolvedMessage[, r: CommandResponder<D>])`;
    - events: `#[derive(EventHandler)]` on a unit struct with an optional `#[event(ordered, droppable)]`, handler `(self, ctx: EventCtx, event: <EventPayload>) -> HandlerResult<()>`.
  - Descriptor names are `concat!(module_path!(), "::", "<Struct>")`. The runtime behaviour of the existing descriptors (`CommandHandler`, `ContextMenuHandler`, `EventHandler`) is unchanged.
- **`#[handler]`.**
  - Takes no arguments. It marks exactly one `async` method per struct, of any name, whose receiver is `self` or `mut self` by value; it must not be generic.
  - It emits, in the same `impl`, a public `fn descriptor() -> <the kind's descriptor>`. It also emits the hidden items that the derive's `Register` impl and the missing-handler check use (see Design Notes).
  - `Module::add::<X>()` (`fn add<T: Register>(self) -> Self`) registers `X::descriptor()`.
  - `Register` is a sealed-by-hidden-items public trait. Its `on_unimplemented` names the kind derives.
  - `Module::add(descriptor)` and the public `Descriptor` trait are removed. The derive-emitted code and core's own unit tests push descriptors through a `#[doc(hidden)] pub fn __add(self, impl Descriptor)`.
- **INT-05.** The 6.10 check moves to the derive's declared defer mode (an associated type) and the `#[handler]` method's responder parameter. The messages are those of Matrix 9.
- **Autocomplete (§8).**
  - `#[option(.., autocomplete)]` is a bare flag.
  - The suggestion method is `#[autocomplete(field)] async fn <any>(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<T>>`, with no `self`. `T` is the field type without `Option`: `i64`, `f64` or `String`.
  - `Module::add::<X>()` registers the command with its methods. The pairing checks of Matrix 15-23 apply.
  - `Suggestions<T>` (new, `noctis-core`):
    - built with `FromIterator<(T, L)>`, `new()`, `push(value, label)`, `len()`, `is_empty()`;
    - labels `L: Into<SuggestionLabel>` from `String`, `&str` and `MessageKey`;
    - a sealed `SuggestionValue` for `i64`, `f64` and `String`.
  - The pipeline lowers `Suggestions<T>` to the type-8 choices:
    - a `MessageKey` label is resolved through the App translator (`AppShared.strings`) in the user's locale, with Story 6.8's fallback rules;
    - more than 25 items: the first 25 are kept, with one warning;
    - a label over 100 is cut with `noctis_rules::len::truncate(label, 100)`, with a warning;
    - an empty label, and any value `validate_autocomplete` rejects, are dropped, with one warning.
  - `typed` is the focused option's raw value: a JSON string verbatim, a JSON number as its JSON text, `""` when absent.
  - On `AutocompleteCtx`:
    - `respond` is removed;
    - `guild_id()` returns `Result<Id<GuildMarker>, HandlerError>`, an error outside a guild;
    - `state::<T>()` returns `State<T>`, and panics with the type name when the state is unregistered.
  - The runtime is otherwise unchanged: the 2.5 s deadline, an empty list on error, panic or deadline, and the Autocomplete Error Sink.
  - `AutocompleteHandler`, `__AutocompleteFor`, `__AutocompleteOutput` and `__autocomplete_for` are removed.
- **Components.**
  - Derives: `#[derive(Button)] #[button("id", <codec args>, defaults(..))]`, and likewise `StringSelectMenu`/`string_select`, `UserSelectMenu`/`user_select`, `RoleSelectMenu`/`role_select`, `MentionableSelectMenu`/`mentionable_select`, `ChannelSelectMenu`/`channel_select`.
  - Each derive implements `Route` + `Payload` (+ `SelectRoute` for every select, which always carries the `N` nonce) with the unchanged CID-01 proof, the id rules (CID-03) and the codec arguments `by_ref`, `overflow = spill`, `schema = n`, `signed`.
  - `select`, `view` and `opens_modal` are unknown arguments (CID-03).
  - `ttl`, `one_shot`, `owner`, `permissions`, `guard`, `rate_limit`, `response` and `auto_defer` are CID-03 runtime arguments "not supported yet" (Epic 7 adds them).
- **`defaults(..)` keys.**
  - Button: `style` (`Primary | Secondary | Success | Danger`), `label`, `emoji`, `disabled`. Required: `style` and `label` or `emoji`.
  - Selects: `placeholder` (required), `min_values`, `max_values`, `disabled`.
  - String select: plus `options = <Choices type>`. Channel select: plus `channel_types = [..]`, with names from twilight's `ChannelType` variants.
  - Modal: `title` (required).
  - Values are literals: text as string literals (no locale keys in any feature set until Story 9.3), integers as integer literals, bools as `true`/`false`.
  - `emoji` is `"<:name:id>"` / `"<a:name:id>"` (custom) or any other non-empty string (unicode).
  - Omitted keys are not sent: Discord's `min_values = 1`, `max_values = 1` and `disabled = false` apply.
  - Error texts are those of Matrix 33-46.
- **`.build()`.**
  - It is an inherent `pub fn build(self)` emitted by the derive. It returns:
    - `RouteButton<Self>` for a Button;
    - `StringSelect` when `options` is declared, else `StringSelectBuilder<Self>`, whose `.options(iter) -> StringSelect` is required before the select can be placed (no `RowSelect`/placement impl; Matrix 47);
    - `UserSelect`, `RoleSelect`, `MentionableSelect` or `ChannelSelect` for the other selects;
    - `Modal` for a modal.
  - The defaults live on `noctis-ui`'s `ComponentRoute` associated const (AD-19). Overrides are the shipped setters: `label`, `emoji`, `style`, `disabled`, `placeholder`, `min_values`, `max_values`, `default_users`/`default_roles`/`default_channels`, `channel_types`, plus a new `StringSelect::options(iter)` that replaces the options.
  - `RouteComponents` is removed. Link and Premium buttons stay plain builders.
- **CID-04.**
  - A component or modal struct without a `#[handler]` method fails at `cargo check` with `[CID-04] `X` has no #[handler] method, so no component can be built for it`. The help is `add impl X { #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> }`, and `ModalSubmitCtx` for a modal.
  - Commands, menus and events without a handler fail with the same text minus the rule id.
  - The derive implements `Handled`.
- **Component and modal handlers** are `(self, ctx: ComponentCtx) -> HandlerResult<Responded>` and `(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded>`.
  - `ComponentCtx` and `ModalSubmitCtx` are new opaque types in `noctis-core`. They mirror `CommandCtx`'s accessors: `module`, `shard`, `guild_id`, `channel_id`, `user_id`, `user_locale`, `guild_locale`, `state`.
  - `ComponentHandler` (`name()`, `route() -> &'static str`, `kind() -> ComponentKind`) and `ModalHandler` (`name()`, `route()`) are stored by `Module::add::<X>()`. `App::build` does not read them yet (Story 7.1).
  - `#[doc(hidden)] __testing::{call_component, call_modal}` run a handler with a given struct value over a detached context.
- **Modals.**
  - `#[derive(Modal)] #[modal("id", <codec args>, defaults(title = ".."))]`. Fields without an input attribute are the Payload, length-proven like a Route. Fields with `#[text_input]`, `#[string_select]`, … are the inputs, with the unchanged MOD-01..07/CV2-04/CV2-09 checks. MOD-02 counts only input and text-display fields.
  - `X { .. }.build()` and the generated `X::blank(<payload fields in order>)` both build the modal with empty inputs (pre-fill is Story 9.4).
  - `ModalForm::from_submission(&ModalSubmission)` decodes the Payload from the submission's `custom_id` with `decode_route::<X>`; any decode error is `SubmitError::Malformed`. It then decodes the inputs as before.
- **`components!`.**
  - A Route item is `<expr>.build()` followed by override setters.
  - The macro counts it as one interactive component. Placement is typed:
    - a one-item row accepts a button or any select (hidden `row_one`);
    - a 2-5 item row requires buttons (hidden `row_button`, whose `on_unimplemented` cites `[CV2-06] a select fills its Action Row alone`);
    - an accessory requires a `SectionAccessory` (CV2-07).
  - The existing checks are kept: six items (CV2-06), literal `.label` over 80 (CV2-08), literal `.options([..])` over 25 (CV2-09), duplicate receiver expressions before `.build()` (CID-02).
  - The old `route.button(..)`/`route.<kind>_select(..)` items are parse errors with the help ``write `Route { .. }.build()` ``.
- **Withdrawn forms.** Each fails with a help showing the struct form. The stubs live in `crates/noctis-macros/src/withdrawn.rs`; their facade/prelude re-exports are `#[doc(hidden)]`, so the API snapshot omits them.
  - free `async fn` with `#[command]`, `#[user_command]`, `#[message_command]`, `#[event]` or `#[handler]`;
  - `#[derive(Options)]`;
  - `#[derive(Route)]` / `#[route]`;
  - `#[option(autocomplete = path)]`;
  - bare `#[autocomplete]`;
  - `#[modal(route = ..)]`.
- **Fixtures.** Every trybuild fixture under `crates/noctis/tests/ui/` and `crates/noctis/tests/types/` that uses an old form is rewritten in the new form and re-blessed. Withdrawn-form fixtures are named `tests/types/withdrawn_<form>.rs`. Every macro unit test follows. A line that must still spell an old form for a negative test carries the comment `// withdrawn form`.
- **Docs and skill in the same change.**
  - `docs/discord-rules.md`: drop every "Shipped as / This release / until Story 6.11" clause in CID-03, CID-04, OPT-10..12, INT-05, INT-06, INT-07, CTX-02, and the §2 Route text near line 430. The CID-04 cell states the struct-level check.
  - The template (every module, `AGENTS.md`) and the Usage Skill (every file): remove every `[planned: Story 6.11]` note and "this release" paragraph, and delete `SKILL.md` §8 "Shipped names until Story 6.11". `.default_values(..)`, modal `.title(..)`/`.value(..)` and pre-fill are marked `[planned: Story 9.3]` / `[planned: Story 9.4]`.
  - AD-19's sentence "which only the `#[handler]` method macro implements" becomes "a kind-derived struct without a `#[handler]` method does not compile (CID-04)".
  - `xtask/public-api.txt` is re-blessed and `SKILL.md`'s `API hash` updated.
  - The `xtask` doctrine lint treats a `#[handler]` method whose parameters include `EventCtx` as an event handler, and every other `#[handler]` method as interactive.

**Never:**
- Component/modal dispatch, Registry checks for Routes, `ComponentResponder`/`ModalResponder` and their runtime arguments (Epic 7).
- Modal pre-fill, `.value(..)`/`.title(..)`, `.default_values(..)`, locale-key defaults (Stories 9.3/9.4, Epic 11).
- State/`Ext` parameter injection, `#[on_expired]`/`#[on_stale]` (later stories).
- Deprecated aliases or a second accepted style. New dependencies. Debug builds.

## I/O & Edge-Case Matrix

The error texts in this table are the required texts. `<S>` stands for the struct name.

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 1 | Unit command | `#[derive(SlashCommand)] #[command("ping", description = "d")] struct Ping;` + `handle(self, ctx)`; `Module::new("m").add::<Ping>()` | `Ping::descriptor()`: name `<mod>::Ping`, paths `["ping"]`, options `[]`; runs end to end | — |
| 2 | Options | fields `user: ResolvedUser`, `reason: Option<String>` | `options()` in field order; `self` holds the decoded values; validators run first (6.2) | missing/mistyped → `OptionError` to the sink (unchanged) |
| 3 | Pattern with a group | `"config (set\|get) prefix"` on `struct Config`, handler `(self, ctx, path: ConfigPath)` | `ConfigPath::{Set, Get}`, `PATHS` in expansion order; the handler gets the variant | — |
| 4 | Path parameter mismatch | group pattern without `path`; plain path with `path: X`; group pattern with `path: OtherPath` | E0277 ``this command's Pattern has a group: take `path: ConfigPath` after `ctx` ``; E0277 ``this command has a single path: remove the path parameter``; E0308/E0277 naming `ConfigPath` | compile error (`tests/types/`) |
| 5 | Command attribute | `#[derive(SlashCommand)]` without `#[command]`; `#[command]` with no literal | ``#[derive(SlashCommand)]` needs `#[command("path", description = "…")]` ``; the 6.1 `MISSING_PATTERN` text | compile error |
| 6 | Struct shape | enum, tuple struct, generic struct for any kind derive; non-unit `UserCommand`/`MessageCommand`; non-unit `EventHandler` | ``<derive> needs a struct with named fields or a unit struct``; `[CTX-02] context-menu commands have no options: make `<S>` a unit struct`; ``#[derive(EventHandler)]` needs a unit struct`` | compile error |
| 7 | Handler missing | kind-derived struct with no `#[handler]`: command; menu; event; Button; select; Modal | E0080 ``<S>` has no #[handler] method …` (commands, menus, events); E0080 `[CID-04] …` with the help above (components, modals) | compile error |
| 8 | Handler shape | `#[handler(x)]`; `&self`; `self: Box<Self>`; non-async; generic method; two `#[handler]` methods; `mut self` | ``#[handler]` takes no arguments; put routing arguments on the kind attribute``; ``a #[handler] method takes `self` by value``×2; ``a #[handler] method must be `async` ``; ``a #[handler] method cannot be generic``; E0592 duplicate definitions; ok | compile error |
| 9 | INT-05 | `auto_defer = false` + `CommandResponder`; `auto_defer = false` + no responder; `CommandResponder<Manual>` without the flag; `auto_defer = true` + `Manual`; `auto_defer = false` + `Manual`; same on `#[user_command]`/`#[message_command]` | E0277 ``[INT-05] `auto_defer = false` is declared, so the handler must take `CommandResponder<Manual>` ``; E0277 ``[INT-05] `auto_defer = false` handlers take `r: CommandResponder<Manual>` ``; E0277 ``[INT-05] this handler takes `CommandResponder<Manual>`, so its attribute must declare `auto_defer = false` ``×2; ok; same | compile error |
| 10 | 6.10 arguments carried over | `response = update`; `response = ephemeral`; `ephemeral`; `opens_modal` on `#[command]` | INT-04 text; `ephemerality()` Ephemeral; the 6.10 unknown-argument texts | compile error / — |
| 11 | Menu | `#[derive(UserCommand)] #[user_command("Report", response = ephemeral)] struct Report;` handler `(self, ctx, target: ResolvedUser)`; `description = ".."`; 32-/33-char name; `MessageCommand` taking `ResolvedUser` | descriptor kind User, ephemerality Ephemeral; `[CTX-02]`; ok / `[CTX-01]`; type error | — / compile error |
| 12 | Event | `#[derive(EventHandler)] #[event(ordered)] struct AwardXp;` handler `(self, ctx: EventCtx, event: MessageCreate)`; no `#[event]`; `#[event(foo)]`; `#[event(ordered, ordered)]`; payload `String` / `Interaction` | `ordered` true, `droppable` false; no flags; the 5.6 unknown/duplicate texts; the re-blessed `event_payload_*` E0277 | — / compile error |
| 13 | Registration | `.add::<Ping>().add::<Ping>()`; `.add::<String>()`; `.add::<Buy>()` (Button) | registered twice (App-build duplicate-path error as before); E0277 ``String` is not a Noctis interaction: derive SlashCommand, UserCommand, MessageCommand, EventHandler, Button, a select kind or Modal and add a #[handler] method``; `ComponentHandler` stored, `App::build` ignores it | — |
| 14 | Autocomplete happy path | `rule: i64` with the flag + `#[autocomplete(rule)]` returning `Suggestions<i64>`; `f64`; `String`; `Option<i64>` field with `Suggestions<i64>` | compile; callback `{"type":8,"data":{"choices":[{"name":..,"value":..}]}}` with JSON int / float / string | — |
| 15 | OPT-11: flag without method | flagged `rule`, no method | E0080 ``[OPT-11] option `rule` is flagged `autocomplete`, but `<S>` has no `#[autocomplete(rule)]` method`` | `OPT-11_autocomplete_without_method` |
| 16 | OPT-11: method without flag | `#[autocomplete(rule)]` on an unflagged `rule` | E0277 ``[OPT-11] `<S>` has no option flagged `autocomplete` for this method`` (note: the field must exist and carry `#[option(.., autocomplete)]`) | `OPT-11_method_without_flag` |
| 17 | OPT-11: unknown field | `#[autocomplete(nope)]` | same E0277 | `OPT-11_method_unknown_field` |
| 18 | OPT-11: type mismatch | `Suggestions<String>` for `i64`; `HandlerResult<Responded>`; `Suggestions<u8>` | E0277 ``[OPT-11] the `#[autocomplete]` method of a `i64` option returns `Suggestions<i64>` ``; same; ``[OPT-11] autocomplete suggestions are `i64`, `f64` or `String` `` | `OPT-11_suggestions_type_mismatch` |
| 19 | OPT-10 / OPT-11 kind | flag on a `Choices` field; flag on `bool` | E0080 `[OPT-10]`; E0080 `[OPT-11]` (6.2 consts, rewritten) | fixtures rewritten |
| 20 | INT-06 | `#[autocomplete(rule, auto_defer = false)]`; `#[autocomplete(rule, response = ephemeral)]` | the 6.10 `[INT-06] …` text | rewritten / new `INT-06_autocomplete_response` |
| 21 | Autocomplete attribute shape | bare `#[autocomplete]`; `#[autocomplete(a, b)]`; `#[autocomplete("rule")]`; `#[autocomplete(rule, foo)]`; method with `self`; non-async; parameters `(ctx)` / wrong order; two methods for `rule` | ``#[autocomplete]` needs the field it suggests for: `#[autocomplete(field)]` `` (help shows the method); ``#[autocomplete(field)]` names exactly one field`` ×2; ``#[autocomplete(field)]` takes no other arguments``; ``an `#[autocomplete]` method takes no `self` ``; `… must be async`; ``an `#[autocomplete]` method takes `(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>)` `` ×2; E0592 | compile error |
| 22 | Withdrawn `autocomplete = path` | `#[option(description = "d", autocomplete = suggest)]` | ``autocomplete` is a bare flag: write `#[option(.., autocomplete)]` and add `#[autocomplete(field)] async fn …(ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<T>>` `` | `tests/types/withdrawn_autocomplete_path` |
| 23 | Boundary passes | flag + method on `i64`, `f64`, `String`, `Option<String>` in one struct | compile (`tests/boundary/opt.rs`) | — |
| 24 | Suggestions count | 0 / 25 / 26 items | `choices: []` / 25 / first 25 + one `warn!` on `noctis::autocomplete` | log |
| 25 | Labels | 100-unit / 101-unit label; 51 × `😀` (102 units); `""`; `MessageKey` | unchanged; `truncate(.., 100)` ending `…` + warn; truncated to ≤100 units ending `…`; dropped + warn; translator text in the user's locale | log |
| 26 | Values | `String` 100 / 101 chars; `i64` ±9007199254740991 / ±9007199254740992; `f64` 1.5 / NaN / ±∞ | kept / dropped; kept / dropped; kept / dropped / dropped (one warn per response naming the count and rule ids) | log |
| 27 | Autocomplete inputs | focused `"value":"-"`; `"value":12`; value absent; another option mistyped | `typed == "-"`; `"12"`; `""`; that field `None` in `filled` | — |
| 28 | Autocomplete failures | method `Err`; panic; past 2.5 s; `ctx.guild_id()?` in a DM; `ctx.state::<Unregistered>()` | empty list + sink (`DeadlineMissed` for the deadline); DM → `Err` → empty list + sink; panic message names the type → empty list + sink | sink |
| 29 | Button defaults | `defaults(style = Primary, label = "Buy")`; `emoji = "➡️"` only; label + emoji; `emoji = "<:pt:123>"`; `emoji = "<a:pt:123>"` | `{"type":2,"style":1,"label":"Buy","custom_id":"0shop.buy:47"}` for `Buy { item: 255 }`; unicode emoji; both; custom emoji id 123; animated | — |
| 30 | Select defaults | `defaults(placeholder = "Pick")`; with `min_values = 0, max_values = 25`; `disabled = true`; string `options = Colour`; channel `channel_types = [GuildText, GuildVoice]` | placeholder only (no min/max/disabled keys); 0/25; `"disabled":true`; the options from `Colour::ALL` (label, value, description); `"channel_types":[0,2]` | — |
| 31 | Modal defaults | `#[modal("bio", defaults(title = "Bio"))] struct BioForm { user: Id<UserMarker>, #[text_input("Bio", style = paragraph, max_length = 300)] bio: Option<String> }` | `BioForm { user, bio: Some(..) }.build()` and `BioForm::blank(user)` give the same JSON: `custom_id` `0bio:<user>`, title `Bio`, an empty input | — |
| 32 | Modal submission | `custom_id` of `bio` + input; `custom_id` of another Route; `custom_id` `"!!"` | `BioForm` with `user` and `bio`; `SubmitError::Malformed` naming route `bio` ×2 | typed |
| 33 | `defaults` missing | Button / select / modal without `defaults(..)`; `defaults()` | ``#[button]` needs `defaults(..)` with `style` and `label` or `emoji` ``; ``… with `placeholder` `` (attribute name per kind); ``#[modal]` needs `defaults(title = "…")` `` | compile error (`tests/types/defaults_*`) |
| 34 | Required keys | Button without `style`; without `label` and `emoji`; select without `placeholder`; modal without `title` | the kind's message as in 33 | compile error |
| 35 | Unknown keys | `colour = ..` on a Button; `options` on a user select; `channel_types` on a role select; `label` on a select | ``unknown `defaults` key `colour` for `#[button]`: expected `style`, `label`, `emoji` or `disabled` `` (lists per kind) | compile error |
| 36 | Duplicate / non-literal | `label = "a", label = "b"`; `label = LABEL`; `min_values = n`; `disabled = "yes"` | ``duplicate `defaults` key `label` ``; ``defaults` values are literals: `label` takes a string literal``; `… an integer literal`; ``disabled` takes `true` or `false` `` | compile error |
| 37 | Style | `style = Link`; `style = Premium`; `style = Blurple` | ``Link and Premium buttons have no handler: build them with `LinkButton::new(url)` / `PremiumButton` `` ×2; ``style` takes `Primary`, `Secondary`, `Success` or `Danger` `` | compile error |
| 38 | CV2-08 label | 80 / 81 ASCII; 40 / 41 × `😀`; `""` | ok / `[CV2-08]`; ok / `[CV2-08]` (`discord_len`); ``label` is empty: omit it and give `emoji` `` | compile error |
| 39 | Emoji literal | `""`; `"<:x>"`; `"<:x:abc>"`; `"<:x:0>"` | ``emoji` is empty``; ``custom emoji literals are `<:name:id>` or `<a:name:id>` `` ×3 | compile error |
| 40 | CV2-09 select | placeholder 150 / 151; `min_values = 25` / `26`; `max_values = 0` / `1` / `25` / `26`; `min_values = 3, max_values = 2` | ok / `[CV2-09]`; ok / `[CV2-09]`; `[CV2-09]` / ok / ok / `[CV2-09]`; `[CV2-09]` min > max | compile error |
| 41 | Choices options | `options = Colour` with 25 / 26 variants; `options = NotChoices` | ok / E0080 `[CV2-09]`; E0277 ``NotChoices` is not a Choices enum`` | compile error |
| 42 | Channel types | `[GuildText]`; `[]`; `[Bogus]`; `[GuildText, GuildText]` | ok; ``channel_types` lists at least one type``; unknown name listing valid ones; duplicate error | compile error |
| 43 | MOD-01 title | 45 / 46 chars; `""` | ok / `[MOD-01]`; `[MOD-01]` | compile error |
| 44 | Codec arguments | `select` on `#[string_select]`; `view`; `opens_modal`; `ttl = "1m"`; `response = update`; `auto_defer = false`; `by_ref, signed`; `schema = 61` / `62`; `signed, schema = 3, overflow = spill` | CID-03 unknown `select` (help: every select kind carries the nonce); CID-03 unknown `view` / `opens_modal`; CID-03 runtime argument ×3 (the 2.5 text, attribute name per kind); CID-03 by_ref+signed; ok / CID-03; ok | compile error |
| 45 | Route id | `""`, 24 / 25 chars, `"Shop"`, `"_x"`, `"a:b"` on every kind | the CID-03 texts, unchanged, spanned on the literal | compile error |
| 46 | CID-01 width | inline worst case 100 / 101 on a Button; a select whose `N` (2) makes 101; `String` field; `Id` field; modal Payload worst case 101 | ok / E0080 `worst case 101`; E0080; `[CID-01]` unbounded; ok; E0080 | compile error |
| 47 | String select typestate | `TicketSystem.build()` without `options` placed in `components!` / `ActionRow::select`; then `.options(opts)`; `.options(Vec::new())` sent | E0277 ``[CV2-09] this String Select has no options: call `.options(..)` or declare `defaults(options = …)` ``; ok; runtime validator `[CV2-09]` (Strict error / Lenient warning) | compile error / runtime |
| 48 | Overrides | `Buy{..}.build().label("x").style(ButtonStyle::Danger).disabled(true)`; `Pick.build().placeholder("p").max_values(3)`; `Colourful.build().options([o])` | each replaces one field; the rest from the defaults | — |
| 49 | Select id resolution | `serde_json::to_value(Pick.build())` before core resolves | serde error ``custom_id of route `pick` is unresolved`` (select nonce, unchanged) | typed |
| 50 | `components!` | `row [ Buy{item:1}.build() ]`; `row [ a.build(), b.build() ]` (Buttons); 5 / 6 items; `row [ pick.build(), buy.build() ]`; `section [ text("a") ] accessory = pick.build()`; `row [ x.build(), x.build() ]`; `.label("81…")`; `.options([26 items])`; `row [ buy.button() ]` | ok; ok; ok / `[CV2-06]`; E0277 `[CV2-06]`; E0277 `[CV2-07]`; `[CID-02]`; `[CV2-08]`; `[CV2-09]`; ``write `Route { .. }.build()` `` | compile error |
| 51 | Component handler | `impl Buy { #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> }`; extra parameter; `ctx: CommandCtx` | `Buy::descriptor().route() == "shop.buy"`, `kind() == Button`; `__testing::call_component(Buy{item:7})` runs the handler with `self.item == 7`; ``component handlers take `(self, ctx: ComponentCtx)`; responder parameters arrive with Story 7.2``; type error naming `ComponentCtx` | — / compile error |
| 52 | Modal handler | `impl BioForm { #[handler] async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> }` | `descriptor().route() == "bio"`; `call_modal` runs it | — |
| 53 | Withdrawn forms | `#[command("ping", description = "d")] async fn ping(ctx: CommandCtx) ..`; same with `#[user_command]`, `#[message_command]`, `#[event]`; `#[handler] async fn h(ctx: ComponentCtx)`; `#[derive(Options)]`; `#[derive(Route)] #[route("a")]`; `#[modal(route = R, title = "t")]` | an error whose `= help:` shows the struct form (`#[derive(SlashCommand)] #[command("ping", ..)] struct Ping; impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) .. }` and the kind's equivalent) | `tests/types/withdrawn_*` |
| 54 | Unicode / odd input | `#[button("shöp")]`; `label = "Bü"` (2 units); `#[command("ping", descripción = "d")]`; `#[autocomplete(rüle)]` on field `rüle` | CID-03 id rule; ok; unknown argument `descripción`; ok (the field-name hash covers non-ASCII identifiers) | — / compile error |
| 55 | Hygiene | a user type named `Option`, `Result`, `Box`, `Vec`, `String`, `core`, `std` beside any derive | compiles (`derive_hygiene.rs`) | — |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/lib.rs` -- the macro list.
  - Remove the `Route` and `Options` derives and the `command`/`user_command`/`message_command`/`event` attribute macros as real expanders.
  - Add the derives `SlashCommand` (helpers `command`, `option`), `UserCommand` (`user_command`), `MessageCommand` (`message_command`), `EventHandler` (`event`), `Button` (`button`), `StringSelectMenu` (`string_select`), `UserSelectMenu`, `RoleSelectMenu`, `MentionableSelectMenu`, `ChannelSelectMenu`.
  - Rework `Modal` (helpers `modal` + the input attributes).
  - Add the attribute macros `handler` and `autocomplete`, plus the stubs from `withdrawn.rs`.
  - A derive helper shadows a same-named attribute macro, verified on 1.91 and 1.97. So the stub `command` attribute macro and the `SlashCommand` helper `command` coexist in the prelude without E0659.
- `crates/noctis-macros/src/command.rs` (1535 lines) -- reuse `parse_args`, `check_pattern`/`report`, `path_enum`/`variants`, `parse_response`, `parse_auto_defer`, `meta.rs`.
  - Replace `check_params`, `adapter`, `constructor` and `expand(ItemFn)` with a derive expansion over a `DeriveInput`.
  - Rename `enum_name(fn)` to `<Struct>Path`.
  - Tests L834+.
- `crates/noctis-macros/src/options.rs` -- the field parsing, the const checks and `partial_tokens` become the SlashCommand field half.
  - The `autocomplete` argument (L34 `ARGS`, `parse_args` L300, `autocomplete_tokens` L789) becomes a bare flag plus the OPT-11 fallback const; `= path` is withdrawn.
- `crates/noctis-macros/src/menu.rs`, `event.rs` -- from `ItemFn` attributes to derive expansions. Keep the argument parsers (`parse_args`, `check_name`, the CTX-02 text).
- `crates/noctis-macros/src/autocomplete.rs` -- becomes the `#[autocomplete(field)]` method attribute. Keep the INT-06 text L60-66.
- `crates/noctis-macros/src/route.rs` -- `parse_route_attr`/`parse_list`/`parse_arg` (L78-350) generalize to "kind attribute": id, codec args, then `defaults(..)`.
  - `select` is removed from `RouteArgs`; the select kinds set it implicitly.
  - Extend `RUNTIME_ARGS` (L39) with `rate_limit`, `response`, `auto_defer`.
  - Update `ARGS_HELP` (L41).
  - `expand` L353 becomes a shared `route_impls(input, args, kind)`.
- `crates/noctis-macros/src/modal.rs` -- `parse_modal_attr` L228 takes the id, codec args and `defaults(title)` (the `route =`/`title =` form is withdrawn).
  - Fields without an input attribute become the Payload. `check` L795/`validate` L671 skip them for MOD-02.
  - Emit the hidden payload Route type, `blank`, `build` and `from_submission`.
- `crates/noctis-macros/src/components/{parse.rs L146-190, L646-735; lower.rs L197, L247-300, L386, L497, L598-640; expand.rs L77-160}` -- the `.build()` item and kind-agnostic lowering/expansion (Matrix 50).
- `crates/noctis-macros/src/payload.rs` L13-90 -- reused for Payload impls. `common.rs` -- shared error helpers.
- New `crates/noctis-macros/src/{handler.rs, defaults.rs, kind.rs, withdrawn.rs}` -- `#[handler]`; `defaults(..)` parsing and literal checks (reuse `literal.rs` LABEL/PLACEHOLDER and `noctis_rules` limits); the shared kind-derive scaffolding (Register impl, missing-handler const); the stubs.
- `crates/noctis-core/src/module.rs` L1-160 -- `Descriptor` becomes hidden (`__add`); add `Register`, `add::<T>()`, the `components`/`modals` vectors (shown in `Debug`).
- `crates/noctis-core/src/command.rs` L290-560 and `menu.rs` L100-225 -- keep the `__new*` constructors as binding targets (closures from `#[handler]`). The options path takes `Self` as `O`. `from_call`/`with_autocomplete` take the struct's method table instead of `O::autocomplete`.
- `crates/noctis-core/src/handler.rs` L337-360 -- `EventHandler::__new` stays the event binding target.
- `crates/noctis-core/src/autocomplete.rs` -- see below.
  - Remove `respond` (L311), `AutocompleteHandler`, `__AutocompleteFor`/`__AutocompleteOutput`/`__autocomplete_for` (L460-590). Keep `ErasedAutocomplete`, built from the `#[autocomplete]` bindings.
  - Add `Suggestions<T>`, `SuggestionLabel`, `SuggestionValue`, the `guild_id`/`state` changes and the lowering in the pipeline (`collect`/`validate_autocomplete` path).
  - `Options::autocomplete` in `options.rs` L105 becomes a hidden method table on the derived struct.
- New `crates/noctis-core/src/component.rs` (`ComponentCtx`, `ComponentHandler`, `ComponentKind`, `ModalSubmitCtx`, `ModalHandler`) -- reuse `CtxInner::detached_parts` from `command.rs` L96.
- `crates/noctis-core/src/responder.rs` L120-176 -- the INT-05 hidden traits/assertions retarget to the defer-mode associated type.
- `crates/noctis-core/src/lib.rs` L195-330 -- re-exports and `__testing` (`call_component`, `call_modal`).
- `crates/noctis-ui/src/route.rs` -- delete `RouteComponents` (L33-195). Keep `Handled` and `CustomId` (generalize `CustomId::new` to a Route whose Payload is another type, for modals). Add `ComponentRoute`, the kind markers, the defaults structs and the `#[doc(hidden)]` build functions.
- `crates/noctis-ui/src/component/{button.rs, select.rs}` -- `from_id` gains defaults; add `StringSelect::options`, `StringSelectBuilder`.
- `crates/noctis-ui/src/modal/form.rs` L22-90 -- reshape `ModalForm`.
- `crates/noctis-ui/src/placement.rs` L107-125 -- the `row_one`/`row_button` traits (CV2-06 text).
- `crates/noctis-ui/src/lib.rs` -- docs and re-exports (L36, L166, L280).
- noctis-ui tests: `crates/noctis-ui/tests/{components.rs, messages.rs, modal.rs, validate.rs}` and the snapshots -- they hand-implement `Route`/`Handled`/`ComponentRoute` (noctis-ui cannot use the macros).
- `crates/noctis/src/lib.rs` -- see below.
  - Rewrite every crate doc and example: L47-80, L219-400, L490-670 (Options, autocomplete), L745-985 (command/menus/events), L1142-1200, L1327-1375 (prelude), L1500-1920.
  - Re-export the derives, `handler` and `autocomplete`, the hidden stubs, `Suggestions`, `ComponentCtx`, `ModalSubmitCtx`, `ComponentHandler`, `ModalHandler`, `ComponentKind`, `Register`.
- Facade tests: `crates/noctis/tests/{autocomplete.rs, commands.rs, components.rs, components_macro.rs, derive.rs, derive_hygiene.rs, events.rs, menus.rs, merge.rs, messages.rs, modal_derive.rs, options.rs, validate.rs, error_sinks.rs, boundary/*.rs}`.
- Testkit tests: `crates/noctis-testkit/tests/{commands.rs, events.rs, observability.rs, publish.rs, shutdown.rs}`.
- Core unit tests that call `Module::add(descriptor)`: `app/tests.rs`, `app/tests/commands.rs`, `registry/**/tests.rs`, `pipeline/tests.rs`, `publish/tests.rs`, `observe/tests.rs`, `worker/tests.rs`, `responder/tests.rs`, `edge/tests.rs`. Switch them to `__add`.
- trybuild: `crates/noctis/tests/ui/{cid,cmd,ctx,cv2,int,meta,mod,opt}/*.rs` (every fixture listed by `ls`, 6.10 included) and `crates/noctis/tests/types/*.rs`.
- `docs/discord-rules.md` -- L111-113 (OPT-10..12), L133 (CTX-02), L160-162 (INT-05..07), L176-177 (CID-03/04), ~L430 (§2), the fixture list ~L565.
- `xtask/src/skill_check/doctrine.rs` -- L7-12 doc and the handler detection; tests L487-570 use the struct form.
- `xtask/src/{api_snapshot.rs L759-774, public_api.rs L439-452}` -- test strings using `derive(Route)` switch to `derive(Button)`.
- `xtask/src/skill_check/components.rs` -- `BUILDERS` (add `StringSelectBuilder` if public).
- Template: `examples/project-template/src/modules/{dashboard,profile,setup}/{mod.rs,handlers.rs,events.rs,view.rs}` and `AGENTS.md` L43-55. The skill: `SKILL.md` (L48, §8 L453+, `API hash`), `references/{commands.md, routes-payloads-refs.md, components-v2.md, modals.md, modules-middleware-ratelimits.md, app-and-sharding.md, responder.md, troubleshooting.md, views-and-recipes.md}` (the lines `grep -n "6\.11"` lists).
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` L780-784 (AD-19 sentence).

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-ui/src/{route.rs, component/*.rs, modal/form.rs, placement.rs, lib.rs}` and `crates/noctis-ui/tests/*` -- `ComponentRoute`, defaults, the build functions, `StringSelectBuilder`/`options`, the reshaped `ModalForm`, `row_one`/`row_button`; remove `RouteComponents`; tests for Matrix 29-31, 47-49 by hand-implemented traits. Done first, because the macros emit calls into it.
2. `crates/noctis-core/src/{module.rs, component.rs, autocomplete.rs, options.rs, command.rs, menu.rs, handler.rs, responder.rs, pipeline.rs, lib.rs}` and the core unit tests -- `Register`/`add::<T>()`/`__add`, the ctx and descriptor types, `Suggestions` and its lowering, the `AutocompleteCtx` changes, the INT-05 traits; unit tests for Matrix 24-28, 51-52.
3. `crates/noctis-macros/src/*` -- the kind derives, `defaults`, `#[handler]`, `#[autocomplete(field)]`, the stubs, the `components!` items; macro unit tests for every parse-level row of Matrix 4-13, 15-22, 33-45, 50, 53-54.
4. `crates/noctis/src/lib.rs` -- re-exports, the prelude, docs and doctests in the struct form.
5. Facade, testkit and boundary tests -- end-to-end Matrix 1-3, 11-14, 23, 24-28 (via `__testing::call_autocomplete` and the testkit), 29-32, 46, 51-52, 55.
6. `crates/noctis/tests/{ui,types}/**` -- rewrite every fixture in the new form. Add `OPT-11_autocomplete_without_method`, `OPT-11_method_without_flag`, `OPT-11_method_unknown_field`, `OPT-11_suggestions_type_mismatch`, `INT-06_autocomplete_response`, `CID-04_route_without_handler` (as a Button), `CV2-09_string_select_without_options`, `CV2-08_defaults_label_81`, `CV2-09_defaults_placeholder_151`, `MOD-01_title_too_long` (defaults form), `tests/types/{defaults_missing, defaults_unknown_key, handler_on_free_fn, handler_twice, command_path_param, withdrawn_command, withdrawn_user_command, withdrawn_event, withdrawn_options, withdrawn_route, withdrawn_autocomplete_path, withdrawn_modal_route}`. Delete `types/{autocomplete_not_a_handler, autocomplete_wrong_options, autocomplete_wrong_return, command_options_not_derived}` (superseded). Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui` and read every changed `.stderr`.
7. `docs/discord-rules.md`, the AD-19 sentence, `xtask/src/{skill_check/doctrine.rs, api_snapshot.rs, public_api.rs, skill_check/components.rs}` with their tests.
8. The template and the Usage Skill (Boundaries), then `cargo run --release -p xtask -- api-snapshot --bless` and the `API hash` line.
9. The grep in Verification, then every gate.

**Acceptance Criteria:**
- **Slash command end to end.** Given `#[derive(SlashCommand)] #[command("warn", description = "d", response = ephemeral)] struct Warn { #[option(description = "u")] user: ResolvedUser, #[option(description = "r", autocomplete)] rule: i64 }` with its `#[handler]` and `#[autocomplete(rule)]` methods, registered by `Module::new("m").add::<Warn>()` in a testkit App with `FakeDiscordHttp`:
  - when the command interaction arrives, the handler runs with the decoded `self`;
  - when an autocomplete interaction for `rule` arrives, exactly one type-8 callback carries the method's suggestions as integer values.
- **Context menu and event end to end.** Given `#[derive(UserCommand)] #[user_command("Report", response = ephemeral, auto_defer = false)] struct Report;` taking `CommandResponder<Manual>`, and `#[derive(EventHandler)] #[event(ordered)] struct AwardXp;`, both added with `add::<T>()`:
  - the menu's `r.defer_reply()` sends `{"type":5,"data":{"flags":64}}`;
  - `AwardXp`'s handler receives the gateway `MessageCreate` in per-guild order.
- **Components and modals.** Given `#[derive(Button)] #[button("shop.buy", defaults(style = Primary, label = "Buy"))] struct Buy { item: u64 }` with a `#[handler]`, when it is built and registered, then:
  - `Buy { item: 255 }.build()` serializes to the Matrix 29 JSON;
  - `Module::add::<Buy>()` compiles and stores one `ComponentHandler` for `shop.buy`;
  - the same holds for every select kind and for `#[derive(Modal)]`, with their JSON (insta snapshots).
- **trybuild.** Given the trybuild suite, when it runs, then every fixture matches its blessed `.stderr`, each rule-named `.stderr` contains its `[RULE-ID]`, and each `withdrawn_*` `.stderr` contains a `= help:` with `#[derive(`.
- **Docs, rules and skill.** Given `docs/discord-rules.md`, the template and the skill, when `xtask rules-catalog` and `xtask skill-check` run, then both pass, and no `[planned: Story 6.11]`, "until Story 6.11" or §8 "Shipped names" text remains.
- **Old forms gone.** Given the repository, when the Verification grep runs, then it prints nothing.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Missing-handler check: an inherent item shadows a blanket-trait fallback** (tested on 1.91 and 1.97 in a scratch crate).
  - `#[handler]` emits, inside the user's `impl`, `#[doc(hidden)] pub const __NOCTIS_HAS_HANDLER: bool = true;`, the public `fn descriptor()` and hidden binding fns.
  - `noctis-core` has a hidden `trait __NoHandler { const __NOCTIS_HAS_HANDLER: bool = false; /* fallback fns */ }` with `impl<T> __NoHandler for T {}`.
  - The derive emits:
    ```rust
    #[allow(unused_imports)]
    const _: () = { use ::noctis::__private::core::__NoHandler as _;
        if !<Buy>::__NOCTIS_HAS_HANDLER { panic!("[CID-04] `Buy` has no #[handler] method, so no component can be built for it") } };
    ```
    It is an E0080 under `cargo check`, like CID-01. Its `Register` impl calls `<Buy>::descriptor()` under the same `use`, so a missing handler gives only that one error.
  - Two `#[handler]` methods give E0592 on the hidden const. Accepted: one id, one handler.
- **Autocomplete pairing** uses the same pattern per field. The derive emits a hidden per-struct fallback trait with one `__NOCTIS_AC_<field>` const per flagged field (Matrix 15).
  - The derive also implements `__AutocompleteField<{ fnv1a64("<field>") }>` with `type Value = <field type without Option>` for each flagged field.
  - `#[autocomplete(f)]` binds its method through a function bounded by `Self: __AutocompleteField<{hash("f")}>` (`on_unimplemented` → Matrix 16/17) and `R: __SuggestionsFor<Value>` (→ Matrix 18).
  - The macro writes the wrapper closure itself (`move |ctx, typed: String, filled| async move { Self::m(ctx, &typed, filled).await }`). This avoids a higher-ranked bound over the borrowed `&str`.
  - `#[handler]` likewise classifies its parameters syntactically: a type whose last segment is `CommandResponder` is the responder; the parameter after `ctx` is otherwise the path, target or payload. It calls an arity-specific binding on the hidden kind trait the derive implements (`type Ctx`, `type Defer`, `type Path`, `type Descriptor`), so type errors name the expected types.
- **Why `.build()` is inherent:** no import is needed and the call works in `components!`. Why the Button keeps `RouteButton<Self>`: the existing placement traits and snapshots stay valid.
- **Modal Payload:** the derive emits a hidden payload struct implementing `Route<Payload = Self>`/`Payload`/`PayloadDecode` with the modal's id and codec arguments. `X: Route` names it, and `CustomId` erases that value.
- **Scope cuts, logged as decisions:**
  - component and modal handlers have no responder parameter until Story 7.2;
  - modal `.build()` does not pre-fill until Story 9.4;
  - `.default_values(..)` and the modal `.title`/`.value` overrides come with Story 9.3;
  - defaults are literal text until Story 9.3 adds locale keys.

  Each later story extends the same derive, so nothing here is thrown away.

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok (API hash matches)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rnE 'derive\((Options|Route)\)|#\[route\(|\.button\(\)|Module::add\(|\.add\([a-z_][a-z0-9_]*\(\)\)|\.(string|user|role|mentionable|channel)_select\(|RouteComponents|AutocompleteHandler|autocomplete = [a-z_][a-z0-9_:]*[,)]|planned: Story 6\.11|until Story 6\.11' crates docs README.md examples xtask --exclude-dir=target | grep -v withdrawn` -- prints nothing (lines or paths containing `withdrawn` are the required negative tests and stubs)

**Manual checks:**
- No CI job change: `.github/workflows-disabled/ci.yml` already runs every gate above.
