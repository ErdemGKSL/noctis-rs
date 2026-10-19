# Testing

Covers: the testkit (`noctis::testkit`), simulated commands and clicks, assertions on
Views, response kind and ephemerality, View JSON snapshots, Limit cost, Template pixel
snapshots, Adapter conformance, `assert_memory_flat`. FR-51. Enable the `testkit`
feature in `[dev-dependencies]` only:

```toml
[dev-dependencies]
noctis = { version = "0.1", features = ["image", "default-font", "i18n", "testkit"] }
insta = { version = "1.49", features = ["json"] }
```

Always run tests with `--release` (`cargo test --release`); never create a debug build.

## 1. Structure for testability

- Put everything in `src/lib.rs` with a `configure(AppBuilder) -> AppBuilder`; `main.rs`
  only calls it. Integration tests in `tests/` import the lib.
- Keep `View::render` pure so Views can be rendered from plain data in unit tests.

## 2. A TestApp [planned]

```rust
use noctis::id::Id;
use noctis::prelude::*;
use noctis::testkit::{ResponseKind, TestApp, TestGuild, TestUser};

fn app() -> anyhow::Result<TestApp> {
    Ok(TestApp::build(configure(App::builder()))?      // runs every App-build check, no network
        .guild(TestGuild::new(GUILD, "Test Guild").text_channel(LOGS, "logs").role(MODS, "Mods"))
        .user(TestUser::new(ADMIN).permissions(Permissions::MANAGE_GUILD))
        .user(TestUser::new(MEMBER)))
}
```

`TestApp` uses memory Adapters (overridable), a fake Discord cache seeded from
`TestGuild`, a fixed clock (`app.advance(Duration)` for TTLs and rate limits), and the
real pipeline (decode, guards, Middleware, auto-defer timing, validator).

## 3. Driving interactions

```rust
let reply = app.command("setup start").by(ADMIN).in_guild(GUILD).run().await?;
let reply = app.command("profile").option("user", MEMBER).by(ADMIN).in_guild(GUILD).run().await?;
let reply = app.user_command("Show profile").target(MEMBER).by(ADMIN).in_guild(GUILD).run().await?;

let next  = app.click(reply.button("Next")).by(ADMIN).run().await?;           // by label
let page2 = app.click(reply.button_by_emoji("▶")).by(ADMIN).run().await?;
let picked = app.select(reply.component::<PickLogChannel>()).channels([LOGS]).by(ADMIN).run().await?;
let modal = app.click(reply.component::<EditBio>()).by(ADMIN).run().await?;   // ResponseKind::Modal
let saved = app.submit(modal.modal::<BioForm>().text("bio", "hi").radio("pronouns", "they")).by(ADMIN).run().await?;
let auto  = app.autocomplete("mod warn", "rule", "spa").by(ADMIN).run().await?;
app.event(MessageCreate::test(GUILD, MEMBER, "hello")).await?;
```

## 4. Assertions on a response

| Method | Returns |
| --- | --- |
| `kind()` | `ResponseKind::{Reply, Update, Deferred, Modal, Autocomplete, None}` |
| `is_ephemeral()` | bool |
| `text()` | all Text Display content joined (for `contains` checks) |
| `button(label)`, `button_by_emoji(e)`, `component::<R>()` | handles to click; `is_disabled()`, `style()` |
| `components()` | flattened list |
| `view_json()` | the exact JSON Noctis would send (for `insta::assert_json_snapshot!`) |
| `cost()` | `Cost { components, text_chars, pages }` |
| `attachments()` | names, sizes, alt texts |
| `is_guard_notice()`, `is_rate_limited()`, `is_expired_notice()`, `is_error()` | pipeline outcomes |
| `followups()` | follow-up messages |
| `http_calls()` | what would have gone over the wire (empty when validation failed first) |

Snapshot rule: a View snapshot changes only when its rendered JSON changes (select
nonces are normalized). Review with `cargo insta review`.

```rust
insta::assert_json_snapshot!("setup_step1", step1.view_json());
assert!(roles.cost().components <= 40);
```

## 5. Unit-testing a View without an App

```rust
let view = SetupWizard::new(SetupState { owner: ADMIN, log_channel: None, mod_role: None });
let rendered = noctis::testkit::render_view(&view, Nav::step(0), Locale::EnglishUs)?;
insta::assert_json_snapshot!(rendered.json());
assert!(rendered.cost().components <= 40);
```

## 6. Template pixel snapshots

```rust
let renderer = noctis::testkit::test_renderer()?;   // default font, no network, placeholder assets
noctis::testkit::assert_render_snapshot!(renderer, "profile_card", card, tolerance = 0.01);
```

First run writes `tests/snapshots/profile_card.png`; later runs compare. Runtime class
strings with unknown Tailwind classes fail tests under the testkit.

## 7. Adapter conformance

```rust
#[tokio::test]
async fn bounded_cache_store_conforms() {
    noctis::testkit::conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
```

See `adapters.md` §5 for all suites.

## 8. Memory flatness (NFR-2)

```rust
noctis::testkit::assert_memory_flat(&app, 1_000, |app| async move {
    app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await.map(drop)
}).await?;
```

Fails if framework memory grows with the number of interactions — catches accidental
per-message state.

## 9. What to test in every feature

1. The entry command returns the expected View (snapshot) with the right kind and
   ephemerality.
2. Each author Route: click → expected update; guard rejects other users.
3. Overflow: seed enough data to paginate; assert Navbar and Limits on page 2.
4. Empty state renders.
5. Modal: open, submit, View updated; invalid input gives the notice.
6. Custom Adapters pass conformance.
