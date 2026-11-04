# Registrations defined anywhere

A Noctis bot does not need a central list of its Commands, components, event handlers
and Modules. Each struct names the Module it belongs to on its own attribute, each
feature folder declares its Module once, and one `AppBuilder::discover()` mounts
everything the binary links (FR-81). Collection happens through
[`inventory`](https://crates.io/crates/inventory): before `main`, each entry only links
itself into a static list (no allocation, no I/O, none of your code runs), and Noctis
reads the list only in `build()`.

The [Project Template](../../examples/project-template/) is written this way. The Usage
Skill's [modules reference](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md#1b-discovered-registrations-fr-81)
has the same rules in short form.

## 1. Name the Module on the attribute

Every kind attribute takes `module = "<Module id>"`:

```rust
// src/commands/ping.rs
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", module = "util")]
pub struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_pong()).await
    }
}

// src/shop/buttons.rs
#[derive(Button)]
#[button("shop.buy", module = "shop", defaults(style = Primary, label = "Buy"))]
pub struct Buy {
    pub item: u32,
}

// src/xp/events.rs
#[derive(EventHandler)]
#[event(ordered, module = "xp")]
pub struct AwardXp;
```

It works on `#[command]`, `#[user_command]`, `#[message_command]`, `#[event]`,
`#[button]` and the select attributes, `#[modal]`, `#[hook]` and `#[template]`. The id
follows the Module id grammar (segments of `a-z`, `0-9`, `_` and `-` joined by `/`, at
most 64 characters) and is checked at `cargo check`. Without `module`, the struct is
registered only where you `.add::<T>()` it, as before.

## 2. Declare Module settings in the feature's folder

Anything that belongs to the Module itself (Boot Flags, an App restriction,
Middleware, `base`/`group` metadata, `uses`, locales) goes on a Module value that
`#[noctis::module]` declares:

```rust
// src/shop/mod.rs
#[noctis::module]
fn module() -> Module {
    Module::new("shop")
        .flags(["economy"])
        .before(Middleware::before(guild_only).skip_events().provides::<ShopGuild>())
}

#[noctis::module]
fn admin() -> Module {
    Module::new("shop/admin").apps(["main"])
}
```

The attribute takes a free `fn() -> Module` with no arguments, generics, `async` or
`unsafe`. A Module needs no declaration when it has no settings: discovery creates
`Module::new(id)` for an id nothing declares.

## 3. Discover

```rust
pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    builder
        .locales(strings::catalog())
        .state(BotMeta::new())
        .discover()
}
```

At `build()`, after plugins are installed and before Boot Flags and App restrictions
prune the trees:

1. Declared Modules mount, sorted by id: a root id as a new tree, a nested id under the
   Module of its parent path (mounted, declared or created).
2. Discovered structs merge, sorted by Module id and then type path, into the Module
   with their id anywhere in any tree, after that Module's own `.add` registrations. A
   missing Module is created under its parent path, or as a new root.

From then on the App cannot tell discovered registrations from added ones: Boot Flags,
`apps = [..]`, Module enable and disable, intents, Publish and every build check work
the same way. File order, module order and link order never change the result.

## 4. Mixing with `Module::add`

Both styles mix in one App. A Module mounted with `.module(..)` receives the
discovered structs that name its id. Two mistakes are build errors:

- A struct both added with `.add::<T>()` and discovered:
  `BuildViolation::DuplicateRegistration { type_name, module }`. It stays registered
  once, where it was added. Remove one of the two.
- A Module both declared with `#[noctis::module]` and mounted with `.module(..)`:
  `BuildViolation::DuplicateModule`.

## 5. Several Apps, tests and opting out

- `discover()` is per App. An App that never calls it mounts nothing discovered (one
  `debug` line on `noctis::modules` says how many entries it skipped). Calling it twice
  is the same as once.
- Several Apps in one process each discover everything. Split registrations between
  them with `apps = [..]` on the attribute or `.apps([..])` on the declared Module, as
  with `Module::add`.
- Tests that build the bot's own `configure` (`TestApp::build(configure(&mut
  App::builder()))`) see every registration of the bot's library. A test that wants
  only some Modules builds its App without `discover()` and adds them explicitly.

## 6. Where discovery ends

- Discovery reaches every crate the final binary links. The bot's own library is linked
  into its binary and into each `tests/*.rs` binary, so everything in it is found. A
  crate the binary depends on but never uses may be left out by the linker: reference
  something from it, or mount its Modules explicitly.
- `inventory` supports Linux, macOS, Windows, iOS, Android, FreeBSD and WebAssembly
  targets, among others. On a target it does not support, nothing is discovered and the
  App builds without those registrations (the Commands are then missing from Publish),
  so use `Module::add` there.
- Prefix commands stay explicit: `PrefixPlugin::new("!").command::<T>()`.
