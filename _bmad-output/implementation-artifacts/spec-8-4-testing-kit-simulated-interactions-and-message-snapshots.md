---
title: 'Story 8.4: Testing kit — simulated interactions and message snapshots'
type: 'feature'
created: '2026-10-22'
status: 'done'
baseline_revision: '6acd504ed317ea2f53e817e7e80adf3dd3ffd561'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The project template's tests/flows.rs and tests/memory.rs are not compiled or run by any gate.
    evidence: |-
      The template is excluded from the workspace (Cargo.toml exclude), no AGENTS.md gate runs cargo test in it, and the disabled CI template-build job only runs cargo build with continue-on-error. Its src/ still targets the planned API until Story 14.2, so the reconciled tests cannot type-check yet.
    location: >-
      examples/project-template/tests/flows.rs
    severity: medium
  - summary: >-
      Select handlers cannot read picked values (X7 checks only the sent shape) and pre-filled modal inputs (F5) are unit-tested only.
    evidence: |-
      ComponentCtx select values are still planned and derive modals pre-fill nothing before Story 9.4 (memlog Story 8.4 implementation decision); add end-to-end handler assertions once those APIs land.
    location: >-
      crates/noctis-testkit/tests/testapp.rs
    severity: low
---

<intent-contract>

## Intent

**Problem:** FR-51 promises that a bot author can invoke Commands, click components and submit modals in tests without Discord, and assert the resulting message, response kind, ephemerality, disabled state and Limit cost, snapshot rendered JSON with `insta`, and prove NFR-2 with `assert_memory_flat`. Today only the low-level fakes (`noctis_testkit::fake`) exist; the facade's `testkit` feature is empty, and the template's `tests/flows.rs` and the skill's `testing.md` describe a `[planned]` `TestApp`.

**Approach:**
- `noctis-testkit` gains `TestApp`: it takes the bot's own `&mut AppBuilder` (plugins install as in production), attaches `Runtime::for_test()`, a token and both fakes through the production override points (`http_proxy`, `gateway_url`), builds and runs the real App, and injects every simulated interaction as a `FakeGateway` `INTERACTION_CREATE`, so the real shard parses it into an `Inbound` and pushes it into the real Interaction Lane (AD-10). Responses are read off the `FakeDiscordHttp` wire. No network.
- Core gains two hidden, behaviour-neutral seams in `noctis_core::__testing`: a run-end observer (so a run ends exactly when its pipeline task ends) and an `AppProbe` (command definitions, built-in notice texts, codec masking and route lookup).
- The facade's `testkit` feature re-exports the new API as `noctis::testkit`. Then the template, the Usage Skill, the spine and the API snapshot are reconciled.

## Boundaries & Constraints

**Always:**
- Public API of `noctis-testkit` (crate root, re-exported by `noctis::testkit`; nothing from `fake` is re-exported):
  - `TestApp`: `async fn build(builder: &mut AppBuilder) -> Result<TestApp, TestError>`; by-value setters `guild(TestGuild)`, `user(TestUser)`, `timeout(Duration)` (per-run wait, default 10 s); runs `command(&str) -> CommandRun<'_>`, `user_command(&str) -> MenuRun<'_>`, `message_command(&str) -> MenuRun<'_>`, `click(TestComponent) -> ClickRun<'_>`, `select(TestComponent) -> SelectRun<'_>`, `submit(ModalForm) -> SubmitRun<'_>`, `autocomplete(path: &str, option: &str, typed: &str) -> AutocompleteRun<'_>`; `async fn stop(self) -> Result<(), TestError>`; `pub const DEFAULT_USER: Id<UserMarker>` (= 1, always seeded, name `tester`).
  - Run builders (all `#[must_use]`, each ends in `async fn run(self) -> Result<Response, TestError>`): every one has `by(Id<UserMarker>)` and `locale(&str)` (default `en-US`). `CommandRun`/`MenuRun`/`AutocompleteRun` add `in_guild(Id<GuildMarker>)`, `in_channel(Id<ChannelMarker>)`; `CommandRun`/`AutocompleteRun` add `option(&str, impl Into<TestOption>)`; `MenuRun` adds `target_user(Id<UserMarker>)` / `target_message(Id<MessageMarker>)`; `SelectRun` adds `values<I: IntoIterator<Item = impl Into<String>>>`, `users`, `roles`, `channels` (iterators of ids).
  - Seeds: `TestGuild::new(Id<GuildMarker>, impl Into<String>)` with `text_channel(id, name)`, `channel(id, name, ChannelType)`, `role(id, name)`, `locale(&str)` (default `en-US`); `TestUser::new(Id<UserMarker>)` with `name(..)` (default `user-<id>`), `permissions(Permissions)` (default empty), `roles(impl IntoIterator<Item = Id<RoleMarker>>)`. A later seed with the same id replaces the earlier one.
  - `TestOption` (`From` `&str`, `String`, `i32`, `i64`, `f64`, `bool`, `Id<UserMarker>`, `Id<ChannelMarker>`, `Id<RoleMarker>`).
  - `Response` (`Debug`, `Clone`): `kind() -> ResponseKind`, `callback_type() -> Option<u8>`, `is_ephemeral()`, `message() -> Option<&TestMessage>`, `followups() -> &[TestMessage]`, `modal() -> Result<ModalForm, TestError>`, `suggestions() -> &[(String, String)]`, `is_guard_notice()`, `is_expired_notice()`, `is_error()`, `http_calls() -> &[HttpCall]`, plus delegations to the final message: `text() -> String` (`""` without one), `message_json() -> Value` (`Null` without one), `cost() -> WireCost` (zero without one), `components() -> Vec<TestComponent>`, `buttons() -> Vec<TestComponent>`, `button(&str) -> Result<TestComponent, TestError>`, `button_by_emoji(&str) -> Result<..>`, `component::<R: Handled>() -> Result<..>`.
  - `TestMessage`: the same message accessors, plus `id() -> Id<MessageMarker>`, `raw_json() -> &Value` (exact, unmasked) and `is_ephemeral()`.
  - `TestComponent` (`Clone`): `component_type() -> u8`, `custom_id() -> Option<&str>`, `route() -> Option<&str>`, `label()`, `emoji()` (unicode or custom emoji name), `style() -> Option<ButtonStyle>` (`None` for link/premium and non-buttons), `is_disabled()`, `options() -> Vec<String>` (string select values).
  - `ModalForm`: `custom_id()`, `title()`, setters `text(key, value)`, `values(key, iter)`, `radio(key, value)`, `checkbox(key, bool)` (errors surface at `run`).
  - `ResponseKind` `{ Reply, Update, Deferred, Modal, Autocomplete, None }`; `WireCost { pub components: usize, pub text_chars: usize }` (`Copy`, `Default`, `Eq`); `HttpCall` with `method() -> &str`, `path() -> &str` (after `/api/v10`), `json() -> Option<&Value>`.
  - `TestError` (`thiserror`, `#[non_exhaustive]`, every message actionable) with exactly: `Runtime`, `Io(std::io::Error)`, `Build(noctis_core::BuildError)`, `Start(noctis_core::Error)`, `Stopped`, `Timeout { what: &'static str, after: Duration }`, `Unknown { kind: &'static str, id: u64 }`, `UnknownCommand { path: String }`, `UnknownOption { path: String, option: String }`, `OptionType { option: String, expected: &'static str, found: &'static str }`, `InvalidOption { option: String, reason: &'static str }`, `MissingOption { option: String }`, `NotAChoice { option: String, value: String }`, `NotAutocomplete { option: String }`, `NotFound { what: String, available: Vec<String> }`, `WrongComponent { expected: &'static str, found: &'static str }`, `Disabled { custom_id: String }`, `NotInteractive`, `NotVisible { user: u64 }`, `SelectValues { min: u8, max: u8, got: usize }`, `NotAnOption { value: String }`, `UnknownInput { key: String }`, `WrongInput { key: String, expected: &'static str }`, `InvalidInput { key: String, reason: String }`, `Unsupported(&'static str)`, `Render(String)`, `TooFewRuns { n: usize }`, `ProfilerBusy`, `AllocatorMissing`, `MemoryGrew { before: u64, after: u64, runs: usize }`.
  - `assert_memory_flat`, `MemoryReport`, `MEMORY_FLAT_SLACK_BYTES`, `render_json(&V2Message) -> Result<Value, TestError>`, and `pub use dhat;`.
- `TestApp::build` order: check the current tokio runtime is multi-thread (else `TestError::Runtime`); start `FakeDiscordHttp::start_with(GatewayBot { shards: 1, .. })` and `FakeGateway::start()`; install the fallback responder; call `.runtime(&Runtime::for_test())`, `.token(..)`, `.http_proxy(http.url())`, `.gateway_url(gw.url())`, `.publisher(false)` and the hidden run observer on the caller's builder (after the author's `configure`, so these win); `builder.build()` (`TestError::Build`); take an `AppProbe`; spawn `app.run_until(stop)`; wait until `FakeGateway::live_sessions() >= 1`, failing with `TestError::Start` if the run task ends first or `TestError::Timeout` after the timeout.
- Each run holds an async run lock (runs are serialized), drains stray requests, allocates `n` (from 1), interaction id `1_000_000_000_000_000_000 + n`, token `noctis-test-<n>`, application id `1_100_000_000_000_000_000`, registers a completion oneshot by interaction id, dispatches the interaction (`0` sessions → `TestError::Stopped`), awaits completion within the timeout (`TestError::Timeout`), then takes the request log (`FakeDiscordHttp::take_requests`), clears the gateway frame log, and builds the `Response`. Nothing per run outlives it (the oneshot map entry is removed by the observer or on timeout).
- Interaction JSON follows Discord's gateway shape (as the existing e2e tests send it): guild runs carry `guild_id`, `guild_locale`, `member { user, roles, permissions, joined_at, deaf, mute, flags }`, `channel { id, type }`, `channel_id`, `app_permissions` = `Permissions::all()` bits, `context` 0; DM runs (no `in_guild`) carry `user`, a DM channel whose id equals the user id (type 1), `context` 1. A guild run's channel defaults to the guild's first seeded text channel, else a text channel whose id equals the guild id. Every referenced guild, user, channel and role must be seeded (`TestError::Unknown`). USER/CHANNEL/ROLE/MENTIONABLE values carry `resolved` data built from the seeds.
- Commands are checked against `AppProbe::commands()` (global plus the run guild's scope, the same JSON Publish sends): the base name and `type` (1, 2, 3), subcommand/group words, option names, option types, required options and `choices`. Guild-scoped commands send `data.guild_id`. Integer values for a NUMBER option are sent as numbers. Discord's client-side min/max/length checks are **not** simulated, so server-side validators are reachable.
- `ResponseKind` follows the first callback, with the maintainer-decision equivalences: type 4 → `Reply`; 5 then a `PATCH @original` → `Reply`; 7 → `Update`; 6 then `PATCH @original` → `Update`; 5 or 6 without an edit (including a deleted original) → `Deferred`; 9 → `Modal`; 8 → `Autocomplete`; no callback → `None`. The final message is the callback data, replaced by later `PATCH @original` bodies; a `DELETE @original` leaves none. `is_ephemeral()` reads flag 64 of the callback data (types 4, 5); for `Update` it is the source message's ephemerality.
- `is_guard_notice`/`is_expired_notice`/`is_error` compare the final message and every follow-up, as a single Text Display, with `AppProbe::notice` of `noctis.guard.{failed,owner,permissions}`, `noctis.expired`, and `noctis.error.generic`/`noctis.validation.failed` in the run's locale order (`notice::locale(user, guild)`). Custom-key notices are not recognized (documented).
- `message_json()` = the sent body (callback `data`, or the `PATCH` body) with every `custom_id` passed through `AppProbe::mask_custom_id`: a select nonce becomes `NN`, a Ref body `~RRRRRRRRRRR`, an integrity tag `TTTTTT`; inline unsigned ids are unchanged. `render_json(&msg)` serializes a clone of `msg` whose unresolved ids are resolved with the placeholder nonce/signer/Ref, as `create_body(CreateFlags::empty())`, then masks the same way.
- `cost()` counts the JSON as sent: V2 (flag 32768): every object with a numeric `type` in `components` at every depth (Section texts and `accessory` included; select `options`, gallery `items` and Thumbnail `media` are not components), and the `discord_len` sum of every type-10 `content`; legacy: rows plus their items, and `discord_len(content)`.
- Click/select/submit fidelity: a disabled component is `TestError::Disabled`; a link/premium button is `TestError::NotInteractive`; `click` on a select or `select` on a button is `TestError::WrongComponent`; a component of an ephemeral message used by another user is `TestError::NotVisible`; select counts outside `min_values..=max_values` (defaults 1/1) are `TestError::SelectValues`; a string-select value not among its options is `TestError::NotAnOption`. A click's default user, guild and channel are those of the run that produced the message. The `message` field is the source message as Noctis sent it (its id, channel, flags, unmasked components).
- Modal submit components are Label-wrapped (`{type: 18, component: {..}}`), per input kind: text 4 (`value`), selects 3/5-8 (`values`), radio 21 (`value`), checkbox group 22 (`values`), checkbox 23 (`value: bool`); unset inputs send their pre-filled value or empty. A required text input left empty, or a value outside `min_length..=max_length` by `discord_len`, is `TestError::InvalidInput`; unknown keys are `TestError::UnknownInput`; a setter of the wrong kind is `TestError::WrongInput`; file upload (19) is `TestError::Unsupported`. A form from a component's modal keeps that component's message as the submit's `message`.
- The fallback responder answers: `POST /interactions/{id}/{token}/callback` → 204; `PATCH|GET|DELETE /webhooks/{app}/{token}/messages/@original` → 200 complete message JSON (`DELETE` → 204); `POST /webhooks/{app}/{token}` → 200 message JSON with a fresh id; `PATCH|GET|DELETE /webhooks/{app}/{token}/messages/{id}` likewise. Everything else keeps the fake's Discord-shaped 404. `http_calls()` lists every request of the run window in order.
- `assert_memory_flat(app, n, f)`: `n < 10` → `TestError::TooFewRuns`; another call in progress → `TestError::ProfilerBusy`; start a dhat `testing()` profiler; no allocation recorded for a probe `Box` → `TestError::AllocatorMissing` (hint: `#[global_allocator] static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;`); run `max(n / 10, 10)` warm-up calls, read `curr_bytes`, run `n` calls, read again; growth over `MEMORY_FLAT_SLACK_BYTES` (64 KiB) is `TestError::MemoryGrew { before, after, runs }`, else `Ok(MemoryReport { warm_up, runs, before_bytes, after_bytes })`. The first `Err` from `f` is returned as is. Signature: `pub async fn assert_memory_flat<'a, F, Fut>(app: &'a TestApp, n: usize, f: F) -> Result<MemoryReport, TestError> where F: FnMut(&'a TestApp) -> Fut, Fut: Future<Output = Result<(), TestError>>`.
- No panics, `unwrap` or `expect` outside tests; every finder returns `Result`. Release-only builds; trybuild stays `compile_fail` only.

**Never:**
- No test-only I/O path in core: the App reaches the fakes only through `http_proxy`/`gateway_url`; the hidden seams only observe or read. No change to dispatch behaviour, Limits, rule ids or `docs/discord-rules.md`.
- `noctis::testkit` re-exports nothing from `fake` and names no `noctis_*` path (api-snapshot must pass).
- Out of scope, marked in the skill with their story: Discord-cache seeding of `ctx.cache()` (Story 12.5), `is_rate_limited` (13.4), attachments (9.1), pixel snapshots and `app.renderer()` (10.11), conformance suites (14.1). The skill drops the fixed clock (`app.advance`) and `app.event(..)` lines (no story owns them).
- No `noctis-testkit → noctis-image` or `→ noctis-codec` edge.

## I/O & Edge-Case Matrix

Unless stated: guild `G=10` "Guild" with text channel `C=20` and role `R=30`, users `A=40` (`MANAGE_GUILD`) and `M=41`, an App mounting the test structs of `crates/noctis-testkit/tests/testapp.rs`. Every row is a test there unless marked (U) = unit test in `noctis-testkit/src`, (M) = `tests/testapp_memory.rs`.

**Build and lifecycle**

| # | Input / State | Expected |
|---|---|---|
| L1 | `#[tokio::test(flavor = "multi_thread")]`, valid builder | `Ok(TestApp)`; the HTTP fake logged `GET /gateway/bot`, the gateway fake one session |
| L2 | current-thread runtime | `Err(TestError::Runtime)`, message names `flavor = "multi_thread"`; no fake left running |
| L3 | builder with a duplicate Route | `Err(TestError::Build(_))` carrying the `BuildError` |
| L4 | `configure` installed a Plugin that mounts a `ping` Module | `command("ping")` replies (AD-24) |
| L5 | `configure` set `.publisher(true)` | no `PUT` Publish request is logged |
| L6 | `app.stop().await` | `Ok(())`; the run task ended |
| L7 | handler sleeps past `timeout(200 ms)` | `Err(TestError::Timeout { .. })`; the next run on the same app works |

**Seeds and options** (C = `command`)

| # | Input | Expected |
|---|---|---|
| S1 | `.by(A).in_guild(G)` | `member.permissions` = `MANAGE_GUILD` bits, `member.roles` from the seed |
| S2 | no `.by` | runs as `DEFAULT_USER` (id 1, `tester`) |
| S3 | `.by(Id::new(99))` unseeded | `Err(Unknown { kind: "user", id: 99 })`, nothing dispatched |
| S4 | `.in_guild(Id::new(98))` unseeded / `.in_channel(Id::new(97))` unseeded | `Unknown` guild / channel |
| S5 | guild without channels | channel id = guild id, type 0 |
| S6 | no `.in_guild` | DM: `user` set, `member` absent, channel id = user id, type 1 |
| S7 | same guild id seeded twice | the later seed wins |
| S8 | `.locale("tr")` / default | interaction `locale` `tr` / `en-US`; guild runs carry the guild's locale |
| C1 | one-word `ping` | `data { name: "ping", type: 1 }` |
| C2 | `setup start` (subcommand), `config set prefix` (group) | options nest as type 1 / type 2 + 1 |
| C3 | `setp start` (unknown) / guild command run in another guild | `Err(UnknownCommand { path })`, no dispatch |
| C4 | `.option("reason", "😀 ünï")` STRING | value passes byte-exact |
| C5 | `.option("count", 5)` INTEGER / `.option("ratio", 5)` NUMBER / `.option("ratio", 0.5)` NUMBER | type 4 value 5 / type 10 value 5 / 0.5 |
| C6 | `.option("count", 0.5)` INTEGER / `"x"` for INTEGER | `Err(OptionType { option, expected: "INTEGER", found })` |
| C7 | `.option("count", f64::NAN)` NUMBER, integer `2_i64.pow(53)` INTEGER | `Err(InvalidOption)` (±2^53 is the edge: `2^53 - 1` passes) |
| C8 | `.option("nope", 1)` | `Err(UnknownOption { path, option: "nope" })` |
| C9 | required option missing | `Err(MissingOption { option })` |
| C10 | choice option with a value not in `choices` | `Err(NotAChoice { option, value })` |
| C11 | `.option("user", M)` USER / `.option("channel", C)` / `.option("role", R)` / user id for MENTIONABLE | typed value plus `resolved.{users,members,channels,roles}` |
| C12 | `.option("user", Id::new(99))` unseeded | `Err(Unknown { kind: "user" })` |
| C13 | value over a declared `max_length` | dispatched unchanged; the handler's validation error notice arrives (`is_error()`) |
| C14 | `user_command("Show profile").target_user(M)` / `message_command("Quote").target_message(..)` | type 2 with `target_id` + resolved user/member; type 3 with a resolved message |

**Response classification** (handlers named for what they do)

| # | Handler | `kind()` / `callback_type()` / `is_ephemeral()` / final message |
|---|---|---|
| K1 | `ctx.respond(view("pong"))` | `Reply` / 4 / false / "pong" |
| K2 | `response = ephemeral`, `ctx.respond` | `Reply` / 4 / true |
| K3 | sleeps past `ack_deadline(100 ms)` then `ctx.respond` | `Reply` / 5 / its ephemerality / the PATCH body |
| K4 | button `ctx.update` | `Update` / 7 / source's / updated message, same id as the source |
| K5 | button sleeping past the deadline, `ctx.update` | `Update` / 6 / the PATCH body |
| K6 | handler defers itself and returns without editing | `Deferred` / 5 / message `None` |
| K7 | `auto_defer = false` button opening a modal | `Modal` / 9; `modal()` is `Ok` |
| K8 | `ctx.respond` then two `ctx.reply_text` follow-ups | `Reply`; `followups().len() == 2`, both ephemeral |
| K9 | handler returns `Err` before answering | `is_error()`; `text()` = the generic notice |
| K10 | handler errors after a type-5 defer | `Deferred`, message `None` (original deleted); one ephemeral follow-up; `is_error()` |
| K11 | `owner` guard, clicked by `M` | `is_guard_notice()`, ephemeral, handler never ran |
| K12 | stale schema id clicked | `is_expired_notice()` |
| K13 | a before-Middleware returns `Flow::Stop(None)` | `None` / `None`; message `None` |
| K14 | handler calls a bot-token REST route through `ctx.http()` | that request is in `http_calls()` and was answered 404 |

**Message accessors and snapshots**

| # | Input | Expected |
|---|---|---|
| M1 | Container [T("## Title"), Section([T("a")], accessory button "Go"), ActionRow [Primary "Next", Secondary disabled "Prev", Link "Docs"]] | `text()` = `"## Title\na"`; `buttons()` = Go, Next, Prev, Docs (pre-order); `button("Prev")?.is_disabled()`; `button("Next")?.style() == Some(Primary)`; `button("Docs")?.style() == None` |
| M2 | `button("Missing")` | `Err(NotFound { what: "button \"Missing\"", available: ["Go","Next","Prev","Docs"] })` |
| M3 | two buttons labelled "Next" | the first in pre-order |
| M4 | `button_by_emoji("▶")` / custom emoji named `next` | found / `button_by_emoji("next")` found |
| M5 | `component::<TurnPage>()` | the first component whose `route()` is `TurnPage`'s id; none → `NotFound` |
| M6 | M1's message | `cost() == WireCost { components: 9, text_chars: 9 }` (Container, 2 texts, Section, accessory, row, 3 buttons) and equals `noctis::ui::cost(&msg, &limits)`'s two numbers |
| M7 | `T("😀é")`; a 25-option string select row; a 10-item gallery | text 3; 2 components; 1 component |
| M8 | legacy `LegacyMessage` reply with content "hi" and one row of 2 buttons | `cost() == { components: 3, text_chars: 2 }`; `text() == "hi"` |
| M9 | public reply of a render with a select, a `signed` button and a `by_ref` button | `message_json()` select id `…:NN…`, tag `TTTTTT`, Ref `~RRRRRRRRRRR`; `raw_json()` holds the real ids |
| M10 | the same render sent twice in two runs | equal `message_json()`; `insta::assert_json_snapshot!("render_panel", ..)` passes both times |
| M11 | `render_json(&render_panel())` | equals M9's `message_json()` (public V2 reply) |
| M12 | `render_json` of an empty `V2Message` | `{"flags":32768,"components":[],"allowed_mentions":{"parse":[]}}` |
| M13 | no message (K6) | `text() == ""`, `message_json() == Null`, `cost() == WireCost::default()`, `button(..)` is `NotFound` |
| U1 (U) | masking: inline unsigned `1turn:5` / non-Noctis id `"abc"` | unchanged / unchanged |

**Clicks and selects**

| # | Input | Expected |
|---|---|---|
| X1 | click an enabled button of a reply | the handler runs with the decoded Payload; `message.id` = the reply's id |
| X2 | click a disabled button | `Err(Disabled { custom_id })`, nothing dispatched |
| X3 | click a link button | `Err(NotInteractive)` |
| X4 | `click` on a select / `select` on a button | `Err(WrongComponent { expected, found })` |
| X5 | string select `min 1 max 2`: `values([])`, `["a"]`, `["a","b"]`, `["a","b","c"]` | `SelectValues` / ok / ok / `SelectValues` |
| X6 | `values(["zzz"])` not an option | `Err(NotAnOption { value })` |
| X7 | user select `users([M])`; role select `roles([R])`; channel select `channels([C])`; mentionable `users([M]).roles([R])` | `data.values` + `resolved`; handler sees them |
| X8 | `roles([Id::new(99)])` unseeded | `Err(Unknown { kind: "role" })` |
| X9 | button of an ephemeral reply clicked by `M` (reply was `A`'s) / by `A` | `Err(NotVisible)` / ok |
| X10 | follow-up message's button | `message` = that follow-up |
| X11 | `by_ref` button click | the Ref resolves (handler runs) |
| X12 | select click then a re-render | the new select nonce differs from the clicked one (Story 8.2 holds through TestApp) |

**Modals and autocomplete**

| # | Input | Expected |
|---|---|---|
| F1 | K7's form, `.text("note", "hi")`, submit | the modal handler gets `"hi"`; `ctx.update` works (`Update`), source = the button's message |
| F2 | required text input left unset, no pre-fill | `Err(InvalidInput { key, .. })` |
| F3 | `max_length = 5`: `"12345"` / `"123456"` / `"😀😀😀"` (len 6) | ok / `InvalidInput` / `InvalidInput` |
| F4 | `.text("nope", ..)` / `.checkbox("note", true)` on a text input | `UnknownInput` / `WrongInput` |
| F5 | pre-filled input left unset | the pre-filled value is submitted |
| F6 | radio, checkbox group, checkbox inputs set | types 21/22/23 shapes; the handler decodes them |
| F7 | modal opened from a slash command; the submit tries `ctx.update`, then on `UpdateUnavailable` answers `ctx.respond` | the submit has no `message`; `Reply` / 4 |
| F8 | `modal()` on a non-modal response | `Err(NotFound { what: "modal", .. })` |
| A1 | `autocomplete("warn", "rule", "sp")` | `Autocomplete` / 8; `suggestions()` as `(value-as-string, label)` |
| A2 | typed `""` | dispatched with `value: ""` |
| A3 | option without the `autocomplete` flag / unknown option | `Err(NotAutocomplete { option })` / `UnknownOption` |
| A4 | the method returns 30 suggestions | 25 in `suggestions()` |

**Memory** (M = one `#[test]` in its own binary with the dhat global allocator)

| # | Input | Expected |
|---|---|---|
| Mem1 (M) | 200 runs of a `ctx.respond` command | `Ok(MemoryReport { runs: 200, warm_up: 20, .. })` |
| Mem2 (M) | a handler pushing 1 KiB into a `State<Mutex<Vec<Vec<u8>>>>` per run, 200 runs | `Err(MemoryGrew { .. })` |
| Mem3 (M) | `n = 9` / `n = 10` | `TooFewRuns` / ok |
| Mem4 (M) | two calls joined concurrently | one `ProfilerBusy` |
| Mem5 | `testapp.rs` (no dhat allocator) calls it | `Err(AllocatorMissing)`, message names `dhat::Alloc` |
| Mem6 (M) | `f` returns `Err(e)` on run 3 | that `e` is returned |

</intent-contract>

## Code Map

- `crates/noctis-testkit/src/lib.rs` (20 lines): crate docs + `pub mod fake;`. Add modules `app` (TestApp, runs, fallback responder, interaction JSON), `response` (Response, TestMessage, TestComponent, ModalForm, WireCost, HttpCall, ResponseKind), `seed` (TestGuild, TestUser, TestOption), `memory` (assert_memory_flat), `render` (render_json), `error` (TestError); re-export them at the root; `pub use dhat;`.
- `crates/noctis-testkit/src/fake.rs`:
  - `FakeDiscordHttp` L424; routes are exact `(Method, path)` (L408 `State.routes`, `lookup` in `fake/http.rs` L343, 404 at L363). Add `fallback_with<F>(&self, F) -> &Self` (used when no route matches, before the 404) and `take_requests(&self) -> Vec<RecordedRequest>` (drains with `mem::take`).
  - `attach` L667 is the h2c helper; TestApp uses plain `http_proxy` (HTTP/1.1), so the author's `HttpConfig` is kept.
- `crates/noctis-testkit/src/fake/gateway.rs`: `dispatch` L567 counts live sessions; add `live_sessions(&self) -> usize`. `clear_frames` L655.
- `crates/noctis-testkit/Cargo.toml`: add normal deps `noctis-ui`, `noctis-rules`, `dhat`, `twilight-model`, `tokio` features `sync`/`time` (present). All are already workspace deps (`dhat = "0.3"`, lock 0.3.3).
- `crates/noctis-core/src/worker.rs` L130-160 `Workers::interaction`: wrap each spawned pipeline future (and the unhandled branch) so the App's run observer, if any, is called with the interaction id when the task ends; use a drop guard so a panic or cancellation still reports.
- `crates/noctis-core/src/app.rs`: `AppShared` gains `run_observer: Option<Arc<dyn Fn(Id<InteractionMarker>) + Send + Sync>>`, set from a new `AppBuilder` field (default `None`) in `build()` (L2011).
- `crates/noctis-core/src/lib.rs` L286 `pub mod __testing`: add
  - `observe_runs(builder: &mut AppBuilder, observer: Arc<dyn Fn(Id<InteractionMarker>) + Send + Sync>)`;
  - `AppProbe::of(&App)` (holds `Arc<AppShared>` + `Arc<Registry>`) with `commands(&self) -> Vec<(Option<Id<GuildMarker>>, Value)>` (each scope through `publish::json::local`, `publish/json.rs` L14, made `pub(crate)`; scopes from `registry.commands().scopes()`, `registry/commands.rs` L202) and `notice(&self, key: &str, user_locale: Option<&str>, guild_locale: Option<&str>) -> String` (`notice::text(strings, notice::locale(..), key)`, `notice.rs` L12-20);
  - `mask_custom_id(raw: &str) -> Cow<'_, str>` and `route_of(raw: &str) -> Option<&str>`, forwarding to the codec.
- `crates/noctis-codec/src/layout.rs`: `decode` L654, `Decoded` L336 (`nonce()` L365). Add `#[doc(hidden)] pub fn mask_volatile(raw: &str) -> Option<String>` using the layout constants; unit-test it there (inline, select, signed, by-Ref, `CodecError`).
- `crates/noctis-ui/src/route.rs`: `CustomId` L298, `resolve_for_send` L370, `duplicate_key`'s placeholder parts L396-410 (`PlaceholderSign`), `finish`. Add `#[doc(hidden)] pub fn resolve_placeholder(&mut self) -> Result<(), EncodeError>` (nonce `SelectNonce::new(0)`, `PlaceholderSign`, a Ref slot finished with the all-`0` Ref). `V2Message::custom_ids_mut` is at `message/mod.rs` L118; `create_body` L124.
- Patterns to copy: `crates/noctis-testkit/tests/responder_ergonomics.rs` (interaction JSON L325-455, fake routes L239-262, readiness L295) and `select_freshness.rs` (`mask_nonces` L333 is superseded by the codec mask). `crates/noctis/tests/memory_flat.rs` (dhat global allocator, one `#[test]`, `curr_bytes`).
- Facade `crates/noctis/Cargo.toml` (`testkit = []` L26): `noctis-testkit = { workspace = true, optional = true }`, `testkit = ["dep:noctis-testkit"]`. `crates/noctis/src/lib.rs`: features list L545-560; add `#[cfg(feature = "testkit")] pub mod testkit` (docs + `no_run` example + explicit `pub use noctis_testkit::{..}` list; no `fake`).
- `xtask/src/deps_check.rs` `GRAPH` L62: add `("noctis-testkit", "noctis-ui")`, `("noctis-testkit", "noctis-rules")`. `xtask/src/api_snapshot.rs` `HIDDEN` L66 already maps `noctis_ui::Handled`.
- Spine `ARCHITECTURE-SPINE.md`: mermaid L59-86 (add `testkit --> ui`, `testkit --> rules`); AD-24 L1090 ("TestApp (Story 8.6)" → 8.4) and L1083-1090 (one sentence: the hidden `__testing` run observer and `AppProbe` only observe; the App reaches the fakes only through its override points); AD-10 L430 unchanged.
- Template `examples/project-template/`:
  - `tests/flows.rs`: real API (`TestApp::build(configure(&mut App::builder())).await?`, `#[tokio::test(flavor = "multi_thread")]`, `?` on finders, `noctis::twilight::model::id::{Id, marker::..}` imports, `.role(..)` loop instead of `roles_named`, a `// [planned, Story 12.5]` note where the Roles tab needs `ctx.cache()` seeding); move `handlers_hold_no_per_message_state` to a new `tests/memory.rs` with the dhat global allocator.
  - `AGENTS.md` L26 tests line: multi-thread runtime, memory test in its own binary.
- Skill `examples/project-template/.claude/skills/noctis/`: `references/testing.md` §2-§4 and §8 (drop `[planned]`, document every item listed in Always, the kind equivalences, masking, `render_json`, the error variants, the out-of-scope `[planned, Story N]` markers); `SKILL.md` API hash L23.
- Memlog: `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` already has the three Story 8.4 planning `(decision)` lines.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-codec/src/layout.rs` -- `mask_volatile` + unit tests -- the codec is the only `custom_id` parser.
2. `crates/noctis-ui/src/route.rs` -- `CustomId::resolve_placeholder` + unit test (select, signed, by-Ref) -- `render_json` of unresolved ids.
3. `crates/noctis-core/src/{app.rs,worker.rs,lib.rs,publish/json.rs}` -- run observer, `observe_runs`, `AppProbe`, `mask_custom_id`, `route_of`; core unit test: the observer fires once per interaction kind (command, autocomplete, component, modal, unhandled ping) and on a panicking pipeline.
4. `crates/noctis-testkit/src/fake.rs`, `fake/gateway.rs` -- `fallback_with`, `take_requests`, `live_sessions` with doctests.
5. `crates/noctis-testkit/{Cargo.toml,src/*}` -- the new modules; unit tests for cost counting, masking use, interaction-JSON shapes (S1, S5, S6, C1, C2, C11), option checks (C5-C10), select/modal checks (X5, X6, F2-F4) and U1.
6. `crates/noctis-testkit/tests/testapp.rs` -- rows L1-L7, S*, C*, K*, M*, X*, F*, A*, Mem5; `insta` snapshot `render_panel` (generate once with `INSTA_UPDATE=always`, review, commit `tests/snapshots/testapp__render_panel.snap`).
7. `crates/noctis-testkit/tests/testapp_memory.rs` -- one `#[test]` (multi-thread runtime built by hand, dhat global allocator) for Mem1-Mem4, Mem6.
8. `crates/noctis/{Cargo.toml,src/lib.rs}` -- the feature wiring and `noctis::testkit` docs.
9. `xtask/src/deps_check.rs` + spine -- the two edges; AD-24 text.
10. Template `tests/flows.rs`, `tests/memory.rs`, `AGENTS.md`; skill `testing.md`.
11. `cargo run --release -p xtask -- api-snapshot --bless`; paste the hash into `SKILL.md` (the memlog already holds the three Story 8.4 planning decisions; add implementation decisions only).
12. Run every Verification gate.

**Acceptance Criteria:**
- Given a bot crate with `noctis = { features = ["testkit"] }` in `[dev-dependencies]`, when a test builds `TestApp::build(configure(&mut App::builder())).await` with seeded guilds, channels, roles and users, then the App runs on `Runtime::for_test()` against `FakeDiscordHttp`/`FakeGateway`, its plugins installed as in production, and every simulated interaction travels FakeGateway → shard → Interaction Lane → pipeline with no other network access.
- Given a TestApp, when a test runs `app.command("setup start").by(user).in_guild(guild).run()`, clicks a returned component by label or Route, selects or submits a modal, then the `Response` reports the message, `ResponseKind` (with the defer equivalences), ephemerality, each button's disabled state and the `WireCost`, equal to `noctis::ui::cost` of the sent message.
- Given a render function, when its output is snapshot-tested with `insta` through `response.message_json()` or `noctis::testkit::render_json`, then the snapshot is the rendered JSON with only per-send volatile id parts masked, identical across runs and between the two paths.
- Given `assert_memory_flat` in a test binary with the dhat allocator, when it runs N interactions, then it passes for a stateless handler and fails with `MemoryGrew` when a handler keeps per-run memory.
- Given the new public API, when the story is done, then `noctis::testkit` exposes only the Always list, `testing.md` documents every item (skill-check mentions pass), `tests/flows.rs` uses only real API or `[planned, Story N]` notes, the API hash matches, and `deps-check` passes with the two new drawn edges.

## Spec Change Log

## Review Triage Log

### 2026-10-21 — Review pass
- verdicts: 42 findings — high 0, medium 4, low 22, false 15, maybe-false 1
- findings:
  - `[low]` `[reject]` (blind) a timed-out run's late requests appear in the next run's `http_calls()` — real (`execute` takes the whole request log), but the spec defines `http_calls()` as every request of the run window and kind/message are token-filtered by `Wire::record`; only reachable after a timeout overlap, and the fix adds filtering logic.
  - `[false]` `[reject]` (blind) a second callback gets 204 not 40060; PATCH @original before ack succeeds — core's edge counts 40060 as answered (`edge/callback.rs:41`), so production does not fail either; `edit_reply`/`fetch_reply`/`follow_up` exist only on `Responder<_, Acked>` (`responder.rs:977`), so an edit before ack cannot be sent.
  - `[low]` `[reject]` (blind) modal selects/radio/checkbox-group submits skip min/max/options/required checks — real, but the spec's modal fidelity list covers text inputs only; a bad submit reaches the handler and surfaces as its error notice; adding the checks means new branches; unlikely in everyday tests.
  - `[low]` `[reject]` (blind) message-component selects accept duplicate values, ignore `channel_types`/`default_values` and guild membership — real; test authors rarely pick invalid values and each check adds guards.
  - `[low]` `[patch]` (blind) `Seeds::channel`/`Seeds::role` fall back across guilds — confirmed at `seed.rs:292-321`; patch: look only in the run's guild when it has one.
  - `[low]` `[reject]` (blind) member permissions/roles are global across guilds — the spec defines `TestUser` permissions/roles once (Always, seeds); multi-guild per-member data needs new public surface; rarely met.
  - `[false]` `[reject]` (blind) message JSON drops embeds/attachments — Noctis bodies never carry embeds or attachments (`noctis-ui/src/message/body.rs` writes `embeds: []` only to clear legacy; attachments are Story 9.1).
  - `[false]` `[reject]` (blind) multipart bodies treated as no body — no Noctis send path emits multipart before Story 9.1 (attachments out of scope per spec Never).
  - `[low]` `[reject]` (blind) `target_message(id)` resolves an empty bot-authored message — real; the spec's API is `target_message(Id<MessageMarker>)` and a content setter would add public surface; the command still runs end to end.
  - `[low]` `[reject]` (blind) a panicked App task maps to `TestError::Stopped` without its cause — real but rare (App panics at start/stop); the spec's error list is exact, so a new variant is new surface.
  - `[low]` `[reject]` (blind) `MEMORY_FLAT_SLACK_BYTES` does not scale with `n` — the 64 KiB slack and `n >= 10` are spec-settled (Design Notes state the ≥330 B/run sensitivity at N = 200); changing it edits the spec.
  - `[low]` `[reject]` (blind) `pub use dhat;` couples dhat's version to the public API — `pub use dhat;` is in the spec's Always list; changing it edits the spec.
  - `[low]` `[patch]` (blind) the template's `dashboard_roles_tab_pages_within_limits` is `[planned, Story 12.5]` but not ignored — confirmed at `flows.rs:98-107`; patch: `#[ignore = "planned, Story 12.5 ..."]`.
  - `[false]` `[reject]` (blind) the skill dropped `app.event`/`app.advance` without markers — the spec's Never list drops them deliberately (no story owns them) and FR-51's consequences do not include events or a clock.
  - `[low]` `[patch]` (blind) run-observer contract gaps (cancellation, `None` path, detached tasks) — the `None` path is every other core test and cancellation is structural (`RunEnd` drop guard, `worker.rs:209-226`); the detached-task limit is real and undocumented; patch: document it in testing.md and the `TestApp` rustdoc.
  - `[false]` `[reject]` (blind) `AppProbe::commands` silently skips invalid guild scopes — scopes are built from `Id<GuildMarker>::get()` (`metadata.rs:133`), never 0, so the `continue` branch is unreachable.
  - `[false]` `[reject]` (edge) second callback answered 204 — see the blind row: 40060 counts as answered in core, and the typestate removes ack methods after an ack.
  - `[false]` `[reject]` (edge) PATCH/GET @original or follow-up before any ack — `edit_reply`/`fetch_reply`/`follow_up` are only on `Acked` responders (`responder.rs:977-1022`).
  - `[low]` `[reject]` (edge) PATCH/GET/DELETE of an unknown follow-up id succeeds — `FollowupHandle` only addresses ids the fake returned; reachable only through raw `ctx.http()` webhook calls; the fix adds a guard.
  - `[false]` `[reject]` (edge) editing the original by explicit message id is dropped — core edits the original only via `@original`; under TestApp the callback id is not even known to core, so no request names it.
  - `[false]` `[reject]` (edge) callback type invalid for the interaction kind accepted — the per-kind responder aliases expose only legal callbacks (`update` only on components/modal submits, `show_modal` gated by INT-05, autocomplete has no responder).
  - `[low]` `[reject]` (edge) stray requests of another run in `http_calls()` — same claim as the blind row; spec defines the run window.
  - `[low]` `[reject]` (edge) duplicate string-select values accepted — see the blind select row.
  - `[low]` `[reject]` (edge) modal select/radio/checkbox-group validation missing — see the blind modal row.
  - `[false]` `[reject]` (edge) legacy Action Row modal inputs echoed as Labels — Noctis modals are Label-only (`noctis-ui/src/modal` has no Action Row) and the spec mandates Label-wrapped submits.
  - `[low]` `[reject]` (edge) `channel_types` ignored for channel options/selects — real; rarely met and adds a guard per path.
  - `[low]` `[patch]` (edge) cross-guild channel/role resolution — same root cause as the blind `Seeds` row; same patch.
  - `[low]` `[reject]` (edge) DM run with `in_channel` of a guild channel has no `guild_id` — real (`wire.rs` `run_channel (None, Some)`); an unusual call, and choosing the error shape is beyond a direct correction (the spec's error list is exact).
  - `[low]` `[reject]` (edge) the same entity option set twice leaves a stale `resolved` entry — real (`command.rs:116-127`), harmless to handlers (they read the option's id), rare, and needs restructuring.
  - `[medium]` `[patch]` (vgap) follow-up edit/delete and `fetch_reply` folding is untested — pre-verified gap; patch: an end-to-end test that edits one follow-up, deletes another and fetches the reply.
  - `[low]` `[patch]` (vgap) run-token isolation after a timeout is never exercised — pre-verified gap guarding L7; patch: make the post-timeout run outlast `sleepy`'s late answer and assert kind/text (not `http_calls()`, which by spec is the run window).
  - `[medium]` `[defer]` (vgap) the template's `tests/flows.rs`/`tests/memory.rs` never compile in any gate — pre-verified; the template's `src/` targets the planned API (pre-existing, until Story 14.2), so no gate can compile it in this change.
  - `[medium]` `[patch]` (vgap other) `*_with_response` methods fail under TestApp — verified: the fallback answers an empty 204 (`wire.rs` `answer`), `CallbackResource::parse` then returns `ResponderError::Transport` (`responder.rs:474-476`); patch: answer 200 with a `resource` body when the query has `with_response=true`, plus a test.
  - `[low]` `[patch]` (vgap other) an edited ephemeral follow-up reports non-ephemeral — verified: `outcome.rs` takes `flagged_ephemeral` of the latest body and edits carry no flag 64; patch: keep the creation body's ephemerality.
  - `[false]` `[reject]` (intent 1) seeds are not App-visible world state — the spec's Never list deliberately leaves `ctx.cache()` seeding to Story 12.5; seeds in the payload fulfil the AC's "builds TestApp with test guilds ...".
  - `[medium]` `[defer]` (intent 2) template reconciliation is textual, not compiled — same root cause as the vgap template row; deferred with it.
  - `[false]` `[reject]` (intent 3) `assert_memory_flat` measures process heap, not framework memory — the spec settles process-wide dhat `curr_bytes` (a superset of framework memory) with a Design Note; framework growth fails it too.
  - `[false]` `[reject]` (intent 4) snapshots depend on masking — the spec deliberately masks only per-send random parts (FR-25, AD-18 keep them random); PRD FR-51 "changes only when its rendered JSON changes" holds for the meaningful JSON.
  - `[maybe-false]` `[reject]` (intent 5) legacy `WireCost` never compared with `noctis::ui::cost` — settle by asserting M8's message against `noctis::ui::cost`; if it differed it would only be low (legacy counting is spec'd at Always "cost()").
  - `[false]` `[reject]` (intent 6) the facade path is not exercised — the facade's `no_run` doctest compiles `noctis::testkit` paths, `cargo hack --each-feature` builds `noctis --features testkit`, and the api-snapshot pins the export list.
  - `[false]` `[reject]` (intent 7) a test seam runs in core's production dispatch path — spec-settled hidden observer, `None` in production; `observe` with `None` only awaits the future (`worker.rs:223-226`); the spine was amended per the spec.
  - `[low]` `[defer]` (intent memlog) X7 handlers cannot read picked select values and F5 is unit-tested only — pre-existing: `ComponentCtx` select values are still planned and derive modals pre-fill nothing before Story 9.4.

## Design Notes

- **Why the FakeGateway path, not a lane handle.** AD-10 says the testkit injects `Inbound` into "the same lanes", and AD-24 forbids a test-only branch. Dispatching `INTERACTION_CREATE` through the fake makes the real shard build the `Inbound` (raw bytes included) and push it, so TestApp exercises exactly the production path; the cost is one local WebSocket frame per run.
- **Why a run observer.** The wire cannot tell when a handler is done (a follow-up may come later). The observer fires when the pipeline task ends, so every request the run made has been answered and logged; a quiet-period heuristic would be slow and flaky. It is `None` in production and changes no behaviour.
- **Why masking, not deterministic nonces.** Select nonces, Refs and tags must stay random per send (FR-25, AD-18); snapshots mask only those parts, through the codec, so a snapshot changes only when the rendered JSON changes.
- **Why `Result` finders.** Library code returns errors for bad input (rust-best-practices); tests use `?` with `anyhow`.
- **Why dhat.** Process-wide `curr_bytes` is the only allocator-level truth without `unsafe` (denied workspace-wide); a 64 KiB slack after warm-up absorbs pool and HPACK noise yet catches ≥ 330 B per run at N = 200. Because dhat is process-global, `assert_memory_flat` belongs in its own test binary with one `#[test]`.
- Memlog `(decision)` lines (logged at planning): (1) TestApp drives interactions through FakeGateway and waits on the hidden run observer; (2) `noctis-testkit` gains drawn edges to `noctis-ui` and `noctis-rules`, codec access goes through `noctis_core::__testing`; (3) volatile `custom_id` parts are masked (`NN`, `~RRRRRRRRRRR`, `TTTTTT`) in `message_json`/`render_json`, and `assert_memory_flat` uses dhat with a 64 KiB slack.

```rust
let app = TestApp::build(configure(&mut App::builder())).await?.guild(TestGuild::new(G, "Guild").text_channel(C, "logs")).user(TestUser::new(A));
let step1 = app.command("setup start").by(A).in_guild(G).run().await?;
assert_eq!(step1.kind(), ResponseKind::Reply);
let next = app.click(step1.button("Next")?).by(A).run().await?;
insta::assert_json_snapshot!("setup_step2", next.message_json());
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (new `testapp.rs`, `testapp_memory.rs`, unit tests, doctests, existing dhat tests)
- `cargo hack test --release --workspace --each-feature` -- all pass (incl. `noctis --features testkit`)
- `cargo run --release -p xtask -- deps-check` -- ok with the two new edges
- `cargo run --release -p xtask -- rules-catalog` -- ok (unchanged)
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt` (install nightly-2026-10-15 + cargo-public-api 0.52.0 first if missing, per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- ok
- `grep -n "\[planned\]" examples/project-template/.claude/skills/noctis/references/testing.md` -- no hit (remaining markers name their story)
- CI: no new gate (the new tests run inside `cargo test`); `.github/workflows-disabled/ci.yml` unchanged

## Auto Run Result

- **Summary:** `noctis-testkit` gains `TestApp`: simulated slash/user/message commands, component clicks/selects and modal submits driven through FakeGateway into the real pipeline, awaited via a hidden core run observer; `Response` finders, `message_json`/`render_json` snapshots with masked volatile `custom_id` parts, seeds (`TestGuild`/`TestUser`), `assert_memory_flat` (dhat, 64 KiB slack); re-exported as `noctis::testkit`.
- **Files changed:** `crates/noctis-testkit/src/app/*` (builders, wire fake, runs, outcome), `response.rs`, `seed.rs`, `render.rs`, `memory.rs`, `error.rs`, `fake/*`; tests `testapp.rs`, `testapp_memory.rs`, snapshot; `crates/noctis-ui/src/route.rs` (test codec hooks); `crates/noctis/` facade feature `testkit`; `xtask/public-api.txt`, `xtask/src/deps_check.rs` (two new edges); project template `tests/flows.rs`, `tests/memory.rs`, `AGENTS.md`, noctis skill `testing.md`.
- **Review findings:** 42 (high 0, medium 4, low 22, false 15, maybe-false 1). Patched 8 (medium 2: follow-up edit/delete/fetch_reply end-to-end test; `*_with_response` answered 200 with `resource` body; low 6: run-guild-scoped `Seeds::channel`/`role`, ignore Story 12.5 template test, document detached-task observer limit, post-timeout run-token isolation test, edited ephemeral follow-up keeps ephemerality, cross-guild resolution). Deferred 3 (template tests not compiled by any gate until Story 14.2, x2; X7 select values / F5 pre-existing). Rejected 31, each with reason in the Review Triage Log (spec-settled scope, unreachable via typestate, or new public surface).
- **Follow-up review recommended:** true (2 medium patched on first pass). Named risk: the new `with_response=true` 200/`resource` fake answer and the follow-up folding paths in `outcome.rs`/`wire.rs` were patched after review and are verified only by the tests added in the same patch.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, test, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** template `tests/flows.rs`/`memory.rs` are reconciled textually only; modal select/checkbox validation and `channel_types` not simulated; `http_calls()` may include a timed-out run's late requests.
