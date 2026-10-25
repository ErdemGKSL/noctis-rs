# Testing

Covers: the testkit (`noctis::testkit`): `TestApp`, seeds, simulated commands, clicks,
selects, modal submits and autocomplete, assertions on rendered messages, response
kind and ephemerality, message JSON snapshots (`render_json`), Limit cost,
`assert_memory_flat`, Template pixel snapshots (`assert_render_snapshot!`, `Tolerance`,
`app.renderer()`), Adapter conformance (`noctis::testkit::conformance`). FR-51. Enable the `testkit`
feature in `[dev-dependencies]` only:

```toml
[dev-dependencies]
noctis = { version = "0.1", features = ["image", "default-font", "i18n", "testkit"] }
insta = { version = "1.49", features = ["json"] }
```

Always run tests with `--release` (`cargo test --release`); never create a debug build.

## 1. Structure for testability

- Put everything in `src/lib.rs` with a `configure(&mut AppBuilder) -> &mut AppBuilder`; `main.rs`
  only calls it. Integration tests in `tests/` import the lib.
- Keep render functions (`render_x(data, &s, ids…) -> V2Message`) pure so they can be
  called from plain data in unit tests.

## 2. A TestApp

```rust
use noctis::Permissions;
use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp, TestError, TestGuild, TestUser};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};

async fn app() -> anyhow::Result<TestApp> {
    Ok(TestApp::build(configure(&mut App::builder()))   // every App-build check, no network
        .await?
        .guild(TestGuild::new(GUILD, "Test Guild").text_channel(LOGS, "logs").role(MODS, "Mods"))
        .user(TestUser::new(ADMIN).permissions(Permissions::MANAGE_GUILD))
        .user(TestUser::new(MEMBER)))
}

#[tokio::test(flavor = "multi_thread")]   // TestApp needs a multi-thread runtime
async fn ping_replies() -> anyhow::Result<()> {
    let app = app().await?;
    assert_eq!(app.command("ping").run().await?.text(), "pong");
    app.stop().await?;
    Ok(())
}
```

`TestApp::build(&mut AppBuilder)` takes your own builder, so your Modules and Plugins
install exactly as in production. It then points the App at a local Discord HTTP fake
and gateway fake (through the App's normal proxy settings), runs it on
`Runtime::for_test()` with `.publisher(false)` (nothing is published), and waits until
its shard is connected. Every run sends one simulated `INTERACTION_CREATE` through the
gateway fake into the **real** pipeline (decode, guards, Middleware, auto-defer timing,
validators, notices); the run ends when the pipeline task ends. Requests a handler
sends from a task it spawned (a detached `tokio::spawn` follow-up) after it returned
are not part of that run's `Response`. Runs are serialized.
Memory Adapters are the App's defaults; any you configure are kept.

- `TestApp::timeout(Duration)`: how long each run waits (default 10 s); past it the run
  is `TestError::Timeout` and the next run still works.
- `app.stop().await`: graceful shutdown. Dropping the `TestApp` also stops the App.
- `TestApp::DEFAULT_USER` (`Id::new(1)`, named `tester`) runs every interaction without
  `.by(..)` and is always seeded.

### Seeds

Every user, guild, channel and role a run references must be seeded, else the run is
`TestError::Unknown { kind, id }` and nothing is sent. A later seed with the same id
replaces the earlier one.

| Seed | Methods |
| --- | --- |
| `TestGuild::new(id, name)` | `text_channel(id, name)`, `channel(id, name, ChannelType)`, `role(id, name)`, `locale("tr")` (Guild Locale, default `en-US`) |
| `TestUser::new(id)` | `name(..)` (default `user-<id>`), `permissions(Permissions)` (the member's permissions in every guild; default empty), `roles([..])` |

A guild run (`.in_guild(G)`) carries the member (permissions, roles), the Guild Locale
and a channel: `.in_channel(C)`, else the guild's first text channel, else a text
channel whose id is the guild id. A run without `.in_guild` is a DM (no member; the DM
channel id equals the user id). Each `TestGuild` seed is fed to the App's Discord
cache (as an available `GUILD_CREATE`: name, Guild Locale, channels, roles) before the
next run, so `ctx.cache().guild(G)`, `.channel(..)` and `.roles(G)` read it under the
configured resources (`adapters.md` §1.2); a re-seeded id is fed again, and with
`DiscordCacheConfig::none()` nothing is fed. Users reach a `USER` cache from the
interactions themselves (a DM run's invoker, resolved user options).

## 3. Driving interactions

```rust
let reply = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
let reply = app.command("profile").option("user", MEMBER).by(ADMIN).in_guild(GUILD).locale("tr").run().await?;
let shown = app.user_command("Show profile").target_user(MEMBER).by(ADMIN).in_guild(GUILD).run().await?;
let quote = app.message_command("Quote").target_message(Id::new(555)).run().await?;

let next   = app.click(reply.button("Next")?).by(ADMIN).run().await?;          // by label
let page2  = app.click(reply.button_by_emoji("▶")?).by(ADMIN).run().await?;   // unicode or custom emoji name
let picked = app.select(reply.component::<PickLogChannel>()?).channels([LOGS]).by(ADMIN).run().await?;
let mode   = app.select(reply.component::<PickMode>()?).values(["fast"]).run().await?;
let modal  = app.click(reply.component::<EditBio>()?).by(ADMIN).run().await?;  // ResponseKind::Modal
let saved  = app.submit(modal.modal()?.text("bio", "hi").radio("pronouns", "they")).by(ADMIN).run().await?;
let auto   = app.autocomplete("mod warn", "rule", "spa").by(ADMIN).in_guild(GUILD).run().await?;
```

Run builders (`CommandRun`, `MenuRun`, `ClickRun`, `SelectRun`, `SubmitRun`,
`AutocompleteRun`) are `#[must_use]` and end in `.run().await`, which returns
`Result<Response, TestError>`. Every one has `by(user)` and `locale("tr")` (the User
Locale, default `en-US`). `CommandRun`, `MenuRun` and `AutocompleteRun` add
`in_guild(..)` and `in_channel(..)`; `CommandRun` and `AutocompleteRun` add
`option(name, value)`; `MenuRun` adds `target_user(..)` / `target_message(..)`;
`SelectRun` adds `values([..])` (string select), `users([..])`, `roles([..])`,
`channels([..])` (entity and mentionable selects).

- **Options** take any `TestOption`: `&str`, `String`, `i32`, `i64`, `f64`, `bool`, or a
  user, channel or role id (`TestOption::{String, Integer, Number, Boolean, User,
  Channel, Role}`). Commands are checked against the definitions Noctis would publish:
  the path words, option names and types, required options and `choices`. An integer
  for a NUMBER option is sent as a number. Discord's client-side min/max/length checks
  are **not** simulated, so your server-side validators run (and send their notice).
- **Clicks and selects** run as the user, guild and channel of the run that produced
  the message, unless `.by(..)` changes the user; the source message is sent exactly as
  Noctis sent it. Refused before sending: a disabled component (`TestError::Disabled`),
  a link or premium button (`NotInteractive`), `click` on a select or `select` on a
  button (`WrongComponent`), another user on an ephemeral message (`NotVisible`), a
  value count outside `min_values..=max_values` (`SelectValues`), a value that is not
  an option (`NotAnOption`).
- **Modal forms** (`ModalForm`, from `Response::modal()`): `custom_id()`, `title()` and
  the setters `text(key, value)`, `values(key, [..])` (selects, checkbox groups),
  `radio(key, value)`, `checkbox(key, bool)`. Unset inputs send their pre-filled value
  or nothing. A required text input left empty or a text outside its length bounds is
  `InvalidInput`; an unknown key is `UnknownInput`; the wrong setter is `WrongInput`; a
  File Upload is `Unsupported` ([planned]). A form opened from a component
  keeps that component's message, so the submit can `ctx.update(..)` it.

## 4. Assertions on a response

| Method | Returns |
| --- | --- |
| `kind()` | `ResponseKind::{Reply, Update, Deferred, Modal, Autocomplete, None}` (below) |
| `callback_type()` | the first interaction callback type (`Some(4)`, …), `None` without one |
| `is_ephemeral()` | the reply's EPHEMERAL flag; for `Update`, the source message's |
| `message()` | `Option<&TestMessage>`: the final message (`None` after a bare defer or a deleted original) |
| `followups()` | `&[TestMessage]`, in send order (edits and deletes applied) |
| `modal()` | `Result<ModalForm, TestError>` (`NotFound { what: "modal" }` otherwise) |
| `suggestions()` | autocomplete `(value, label)` pairs as sent, after truncation to 25 (values as strings) |
| `is_guard_notice()`, `is_expired_notice()`, `is_error()` | the final message or a follow-up is that built-in notice in the run's locale (guard failed/owner/permissions; expired; generic error or validation failed). Custom-key notices are not recognized |
| `is_rate_limited()` | the final message is the default `noctis.rate_limited` notice in the run's locale (any `{retry}` timestamp): a declared `rate_limit(..)` refused the run and the handler never ran. Repeat the same `run` past the limit to trigger it; an `#[on_rate_limited]` or App override message is not recognized, so assert its text |
| `http_calls()` | `&[HttpCall]`: every request of the run (`method()`, `path()` after `/api/v10`, `json()` (the `payload_json` part of a multipart upload), `files()` (`&[RecordedFile]`: the `files[n]` parts, empty for JSON bodies)); REST routes the fake does not know answer 404 |
| `text()` | all Text Display content (legacy: `content`) joined with `\n`; `""` without a message |
| `button(label)`, `button_by_emoji(e)`, `component::<R>()` | `Result<TestComponent, TestError>` (`NotFound` lists what is there) |
| `buttons()`, `components()` | every button / component, pre-order |
| `message_json()` | the JSON Noctis sent, volatile `custom_id` parts masked (below); `Null` without a message |
| `cost()` | `WireCost { components, text_chars }` of the sent JSON (equal to `noctis::ui::cost`); zero without a message |

`TestMessage` has the same message accessors (`text()`, `message_json()`, `cost()`,
`components()`, `buttons()`, `button(..)`, `button_by_emoji(..)`, `component::<R>()`),
plus `id()`, `is_ephemeral()` and `raw_json()` (the exact, unmasked message).
`TestComponent` has `component_type()`, `custom_id()`, `route()` (its Route id),
`label()`, `emoji()`, `style()` (`Option<ButtonStyle>`, `None` for link and premium
buttons and non-buttons), `is_disabled()` and `options()` (string select values).

**Response kinds.** Type 4 is `Reply`; a type-5 defer followed by an edit of the
original is also `Reply` (the auto-defer fired, `callback_type()` is `Some(5)`). Type 7
is `Update`, and so is a type-6 defer followed by an edit. A defer with no edit (or
whose original was deleted, as after an error) is `Deferred`. 9 is `Modal`, 8 is
`Autocomplete`, and no callback at all (a Middleware stop without a notice) is `None`.

**Snapshots.** `message_json()` masks only the per-send parts of each `custom_id`: a
select nonce becomes `NN`, a by-Ref body `~RRRRRRRRRRR`, an integrity tag `TTTTTT`;
inline unsigned ids stay as they are. So a snapshot changes only when the rendered
JSON changes. Review with `cargo insta review`.

```rust
insta::assert_json_snapshot!("setup_step1", step1.message_json());
assert!(roles.cost().components <= 40);
```

**Errors.** Every `TestError` says what to change in the test: `Runtime` (not a
multi-thread runtime), `Io`, `Build` (your builder's `BuildError`), `Start`, `Stopped`,
`Timeout`, `Unknown`, `UnknownCommand`, `UnknownOption`, `OptionType`, `InvalidOption`
(a NaN, or an integer beyond ±2^53 − 1), `MissingOption`, `NotAChoice`,
`NotAutocomplete`, `NotFound` (`what` and the `available` alternatives), `WrongComponent`, `Disabled`, `NotInteractive`,
`NotVisible`, `SelectValues`, `NotAnOption`, `UnknownInput`, `WrongInput`,
`InvalidInput`, `Unsupported`, `Render`, `TooFewRuns`, `ProfilerBusy`,
`AllocatorMissing`, `MemoryGrew`.

**Attachments.** A response with files is recorded as multipart: `call.json()` is its
`payload_json` and `call.files()` lists the uploads, each a `RecordedFile`
(`#[non_exhaustive]`, pub `name` = `"files[n]"`, `filename` = the content name,
`bytes: Bytes`), in body order. The fake answers like Discord: every message it
returns (reply, follow-up, `@original`, the message of a later click) carries
`attachments` with fresh ids, so a later `update` of the same View shows kept entries
(`{"id": "<snowflake>"}`) and no `files()`. The low-level fake has the same view on
`noctis_testkit::fake::RecordedRequest` (`.json()`, `.files()`).

```rust
let reply = app.command("card").run().await?;
let call = reply.http_calls().iter().find(|c| c.path().ends_with("/callback")).unwrap();
assert_eq!(call.files().len(), 1);                       // uploaded once
assert_eq!(call.json().unwrap()["data"]["attachments"][0]["id"], 0);
let again = app.click(reply.button("Same")?).run().await?;
let call = again.http_calls().iter().find(|c| c.path().ends_with("/callback")).unwrap();
assert!(call.files().is_empty());                        // kept by id
```

### The HTTP fake: endpoints the wire does not model

`app.http_fake()` is the `FakeDiscordHttp` (`noctis::testkit::fake`) the App sends to. The
built-in wire answers interactions, messages and the gateway; register answers for
anything else (Create Webhook, Execute Webhook, a raw REST call) before the run:

| Item | What it does |
| --- | --- |
| `.route(Method, path, FakeResponse)` / `.route_with(Method, path, \|&RecordedRequest\| FakeResponse)` | answers `path` (relative to `/api/v10`, compared raw); the last registration wins; a registered route wins over the built-in answers; chainable |
| `.fallback_with(\|&RecordedRequest\| Option<FakeResponse>)` | answers every unmatched path; `None` keeps Discord's `404` |
| `.requests()` / `.take_requests()` / `.clear_requests()` | the recorded requests (copy, drain, clear); `response.http_calls()` is the per-run view |
| `.stall(Method, path, n)` | the next `n` requests to `path` never get an answer (timeout tests) |
| `.black_hole()` / `.close_connections()` | silently stop answering on / close every open connection; each returns the count |
| `.connections()` | `Vec<RecordedHttpConnection>` (`id`, `protocol: HttpProtocol::{Http1, Http2}`, `opened_at`, `closed_at`, `requests`, `pings`, `black_holed`) |
| `.set_gateway_bot(GatewayBot)` | the `GET /gateway/bot` answer: `GatewayBot { url, shards, session_start_limit: SessionStartLimit { total, remaining, reset_after_ms, max_concurrency } }`, `Default`, `.to_json()` |
| `FakeDiscordHttp::start()` / `start_with(GatewayBot)`, `.url()`, `.addr()`, `.attach(&mut builder)` | a standalone fake for an `App` you build yourself; `TestApp` already does this |

- `FakeResponse::json(StatusCode, &Value)`, `FakeResponse::new(StatusCode, bytes)`,
  `FakeResponse::empty(StatusCode)`, `.header(HeaderName, HeaderValue)`, `.delay(Duration)`.
- `RecordedRequest`: pub `method`, `path` (with `/api/v10`), `query`, `headers: HeaderMap`,
  `body`, `connection`; `.route_path()` (the path after `/api/v10`), `.json()`,
  `.files()`. `Method`, `StatusCode`, `HeaderMap`, `HeaderName` and `HeaderValue` are
  re-exported from `http`.
- A webhook recipe (create, execute, assert the query and body) is in `responder.md` §7.

### Lifecycle Hooks

A `TestApp` runs the real App, so its Lifecycle Hooks (`app-and-sharding.md` §7) fire
as in production: `AppStarted`, `ShardReady`, `AllShardsReady` while `TestApp::build`
waits for the shard, `ShardDisconnected`/`ShardResumed` after
`app.gateway_fake().close(4000, "test")`, `ShutdownStarted { reason: Requested }` in
`app.stop()`, which returns after the hook queue drained. There is no Publish (the
TestApp sets `.publisher(false)`). Record what a hook saw through your own state and
assert after `stop()`:

```rust
use std::sync::{Arc, Mutex};
use noctis::lifecycle::{HookCtx, HookEvent, ShutdownStarted};

#[derive(Default)]
struct Seen(Mutex<Vec<&'static str>>);

#[derive(Hook)]
struct RecordShutdown;

impl RecordShutdown {
    #[handler]
    async fn handle(self, ctx: HookCtx, _ev: ShutdownStarted) -> HandlerResult<()> {
        ctx.state::<Arc<Seen>>().expect("registered").0.lock().unwrap().push(ShutdownStarted::NAME);
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_hook_runs() -> anyhow::Result<()> {
    let seen = Arc::new(Seen::default());
    let app = TestApp::build(
        configure(&mut App::builder())
            .state(Arc::clone(&seen))
            .module(Module::new("probe").add::<RecordShutdown>()),
    )
    .await?;
    app.stop().await?;
    assert_eq!(*seen.0.lock().unwrap(), [ShutdownStarted::NAME]);
    Ok(())
}
```

Outside a run the hooks run inline, so a plain built App is enough for the toggle and
Publish hooks: `let app = configure(&mut App::builder()).build()?;` then
`app.modules().set_enabled("profile", ModuleScope::Global, false).await?` has run every
`ModuleToggled` hook when it returns (an unchanged write runs none). A hook's `Err` or
panic goes to the `ErrorOrigin::Hook` sink: register a recording `ErrorSink` with
`.error_sink(ErrorOrigin::Hook, Arc::clone(&sink))` to assert on it.

## 5. Unit-testing a render function without an App

A View is a plain function, so call it:

```rust
let s = Strings::of("en-US");
let st = SetupState { owner: ADMIN, step: SetupStep::Channel, log_channel: None, mod_role: None };
let msg = render_setup(st, &s);
insta::assert_json_snapshot!("setup_step1", noctis::testkit::render_json(&msg)?);
assert!(noctis::ui::cost(&msg, &Limits::default()).components <= 40);
```

`render_json(&V2Message)` serializes the message as a public V2 reply would send it,
with unresolved Route ids filled with placeholder parts and the same masking as
`message_json()`, so it equals the `message_json()` of a public reply of that message
and both paths can share one snapshot.

## 6. Template pixel snapshots

```rust
use noctis::App;
use noctis::image::RemoteImage;
use noctis::testkit::{TestApp, Tolerance, assert_render_snapshot};

#[tokio::test(flavor = "multi_thread")]
async fn profile_card_matches_snapshot() -> anyhow::Result<()> {
    // Your own `configure`: its `.theme(..)`/`.image(..)` install the ImagePlugin, which puts
    // the Renderer on the App exactly as in production (bundled `default-font` or your
    // `ImageConfig::font(..)` fonts: never a font that differs between machines).
    let app = TestApp::build(configure(&mut App::builder())).await?;
    let card = ProfileCard {
        name: "Deniz".into(),
        subtitle: "they/them".into(),
        level_label: "Level 7".into(),
        xp_label: "42 / 100 XP".into(),
        progress: 0.42,
        avatar: RemoteImage::placeholder(), // no network
    };
    assert_render_snapshot!(app.renderer(), "profile_card", card, tolerance = 0.01);
    app.stop().await?;
    Ok(())
}
```

- **Two forms.** `assert_render_snapshot!(renderer, "name", template)` compares exactly
  (`Tolerance::EXACT`); `assert_render_snapshot!(renderer, "name", template, tolerance = t)`
  takes a `Tolerance` or an `f64` ratio (`0.01` = `Tolerance::ratio(0.01)`). The macro
  `.await`s, so it is valid only inside an async fn: use
  `#[tokio::test(flavor = "multi_thread")]` (TestApp needs it).
- **`Tolerance`.** `Tolerance::ratio(r)` (0.0..=1.0, finite, else it panics) is the largest
  share of pixels that may differ; `.channel(d)` makes a pixel count as differing only when
  one of its R, G, B or A values differs by more than `d` (default 0). `Tolerance::EXACT`
  (also `Default`) is ratio 0.0, channel 0. Getters: `max_ratio()`, `channel_delta()`. A
  snapshot matches when both images have the same size and
  `differing <= max_ratio * total` pixels.
- **What is rendered.** `app.renderer()` (`TestApp::renderer()`: the App's `Renderer`
  port, `Option<&Arc<dyn Renderer>>`, `None` without the `image` feature's ImagePlugin or
  `.renderer(..)`) renders the Template through the real Renderer, forced to a still PNG
  with no animation and no upload budget: the view is drawn once with `Frame::STILL`, so a
  JPEG, WebP or animated Template snapshots its still PNG pixels (animations are not
  compared frame by frame). Register the Template on the App
  (`Module::template::<T>()`); an unregistered one fails with the Renderer's
  `UnknownTemplate` error. Without a Renderer it fails with "no Renderer".
- **Handler tests without takumi.** To test a rendering handler without fonts or the
  render pool, build the `TestApp` with `.renderer(FixedRenderer::new(bytes, format))`
  (the template's `adapters::FixedRenderer`, `adapters.md` §2b) instead of
  `.theme(..)`/`.image(..)`: the same Module and handler upload exactly those bytes,
  named `<first 16 hex of job.key(FixedRenderer::ID)>.<ext>`. Never set both (a
  `BuildViolation::DuplicateRenderer`); pixel snapshots need the real Renderer.
- **Files.** The snapshot is `tests/snapshots/<name>.png` of the calling crate. The first
  run writes it (creating the directory); **commit it**. On a mismatch, a size mismatch or
  an undecodable stored file the test panics and writes the actual render to
  `tests/snapshots/<name>.new.png` (compare, then delete it or accept it); a later match
  deletes a stale `.new.png`. Add `*.new.png` to `.gitignore`.
- **Updating.** `NOCTIS_UPDATE_SNAPSHOTS=1 cargo test --release` rewrites every snapshot
  that runs and passes; review the diff before committing.
- **CI.** With `CI` set (GitHub sets `CI=true`), a missing snapshot fails ("missing
  snapshot (CI)") instead of being written, so a forgotten file cannot pass. An
  environment variable counts as on when it is set and not `""`, `"0"` or `"false"`;
  update mode wins over `CI`.
- **Names.** 1..=64 bytes of `[A-Za-z0-9_-]` (no dots, slashes or spaces), checked before
  rendering; keep each name unique in the crate, since all tests share one directory.
- **Determinism.** Use the bundled `default-font` or fonts registered with
  `ImageConfig::font(..)`; both are the same on every machine (avoid any font that
  differs between machines). Use `RemoteImage::placeholder()` for `#[asset]` fields so
  no network is needed, and a small tolerance (`0.01`) absorbs antialiasing differences.
  A downscaled render (the size budget shrank it) fails: snapshots compare the declared
  size. Runtime class strings with unknown Tailwind classes are errors on a test Runtime,
  so they fail tests too.

## 7. Adapter conformance

```rust
#[tokio::test]
async fn bounded_cache_store_conforms() {
    noctis::testkit::conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
```

Each suite is an `async fn` returning `()`: it passes, or it panics on the first failing
case with `` conformance::<suite>: case `<case>` failed: <detail> `` (an unexpected
`Err(PortError)` is a failure whose detail includes the error's text). The factory is
called once per case and must return a fresh, empty store. The suites use the real
clock: run them in `#[tokio::test]` (either flavor), never with `start_paused`. A
byte-store suite takes about 3 s (one 2.5 s wait for 1 s TTLs), `identify_queue` about
5 s. See `adapters.md` §5 for every suite and its cases.

## 8. Memory flatness (NFR-2)

```rust
// tests/memory.rs: its own test binary, with the dhat allocator and exactly one #[test].
#[global_allocator]
static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;

#[test]
fn handlers_hold_no_per_message_state() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async {
        let app = TestApp::build(configure(&mut App::builder())).await?;
        let report = noctis::testkit::assert_memory_flat(&app, 1_000, |app| async move {
            app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await.map(drop)
        })
        .await?;
        assert_eq!(report.runs, 1_000);
        Ok(())
    })
}
```

`assert_memory_flat(&app, n, f)` runs `max(n / 10, 10)` warm-up calls of `f`, reads the
live heap, runs `n` more and reads it again. Growth over `MEMORY_FLAT_SLACK_BYTES`
(64 KiB) is `TestError::MemoryGrew { before, after, runs }`: something keeps per-run
state. Otherwise it returns `MemoryReport { warm_up, runs, before_bytes, after_bytes }`.
The first `Err` of `f` is returned as is. `n < 10` is `TooFewRuns`; a second call while
one runs is `ProfilerBusy`; a binary without the dhat allocator is `AllocatorMissing`.
The heap profiler is process-wide, which is why the memory test gets its own binary.

## 9. What to test in every feature

1. The entry command sends the expected message (snapshot) with the right kind and
   ephemerality.
2. Each author Route: click → expected update; guard rejects other users.
3. Long lists: seed enough data for several pages; assert the tab row, prev/next
   disabled at the bounds and the Limits on every page.
4. Empty state renders.
5. Modal: open, submit, message updated; invalid input gives the notice.
6. Custom Adapters pass conformance.
