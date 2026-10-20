# Noctis bot template

A ready-to-copy Discord bot built on [Noctis](https://crates.io/crates/noctis): a Rust
framework on twilight-rs with built-in sharding, a Components V2 UI kit and Tailwind
image rendering.

> **Status:** written against the planned Noctis API. It will compile once the `noctis`
> crate lands; until then treat it as the reference shape of a Noctis bot.

## What's inside

| Feature | Where | Shows |
| --- | --- | --- |
| `/setup start` | `src/modules/setup/` | Hand-written three-step flow: the step in a Payload field, "Step n of 3" progress, pre-filled selects, Back/Next/Cancel, Finish |
| `/dashboard` | `src/modules/dashboard/` | Hand-written tabs (a `PayloadEnum` field), a role list paginated with the budget helper, module toggles |
| `/profile`, "Show profile" | `src/modules/profile/` | Rendered card (Template) in a Media Gallery, button → modal → re-render bio editing, XP event handler, user command |
| Custom Adapter | `src/adapters/bounded_cache.rs` | Implementing the `CacheStore` port yourself |
| Locales | `locales/en-US.yaml`, `locales/tr.yaml` | Typed strings, word-by-word command translation |
| Tests | `tests/` | Testkit flows, Adapter conformance, pixel snapshot |
| Agent setup | `AGENTS.md`, `.claude/skills/` | Noctis Usage Skill + rust-best-practices |

Every feature folder has the same shape: `mod.rs` (the Module and its Routes),
`view.rs` (render functions, the one place a message is built), `handlers.rs`
(Commands, component and modal handlers, each ending in `ctx.respond(render_x(..))` or
`ctx.update(render_x(..))`) and, when needed, `events.rs`. Noctis has no widgets: tabs,
pages and steps are fields of your own Routes (see the Usage Skill's
`references/views-and-recipes.md`).

## Start a new bot from it

1. Copy the directory out of the noctis-rs repository:

   ```sh
   cp -r noctis-rs/examples/project-template my-bot
   cd my-bot
   ```

2. In `Cargo.toml`, rename the package and switch the `noctis` dependency (both in
   `[dependencies]` and `[dev-dependencies]`) from the path form to the crates.io form:

   ```toml
   noctis = { version = "0.1", features = ["image", "default-font", "i18n"] }
   ```

   Keep the empty `[workspace]` table if the bot is its own repository; delete it if you
   add the bot to an existing workspace.

3. Create a Discord application at <https://discord.com/developers/applications>, add a
   bot, and invite it to a test server with the `bot` and `applications.commands` scopes.

## Configuration

Configuration is code (`src/lib.rs`, `configure`) plus `NOCTIS_*` environment
variables read by `App::from_env()`.

The bot ships as **one self-contained binary**: locale files (`i18n!`), the theme
(`theme!()`), the default font (`default-font` feature) and `.env.example` are compiled
into it, so a deployment needs nothing but the executable. On first run it writes
`.env` next to itself from the built-in `.env.example` (owner-only permissions, a
freshly generated `NOCTIS_PAYLOAD_KEY`) and exits so you can fill in `NOCTIS_TOKEN`.
Later runs load `.env`; variables already exported in the environment win, so a
deployment that sets them directly never needs the file.

Memory: locale strings stay resident, since every interaction uses them. The embedded
`.env.example` is read-only static data that is touched only on first run, so the OS
drops it from memory once it goes cold; every buffer built from it, including the
generated key, is zeroized when it's dropped (`src/bootstrap.rs`).

| Variable | Required | Meaning |
| --- | --- | --- |
| `NOCTIS_TOKEN` | yes | Bot token. Never logged. |
| `NOCTIS_PAYLOAD_KEY` | for `signed` Routes and Clusters | 32-byte key (base64) for `custom_id` integrity tags. Same value on every process. `openssl rand -base64 32`. |
| `NOCTIS_PAYLOAD_KEY_PREVIOUS` | no | Previous key, accepted for verification while you rotate. |
| `NOCTIS_APP_ID` | no | Key namespace for Adapters (`[a-z0-9-]{1,32}`); `configure` sets `noctis-template`. |
| `NOCTIS_DEV_GUILD` | no | Default guild for `--publish-guild` with no id (your test guild's numeric id). |
| `NOCTIS_SHARDS` | no | Shard Range for this process, e.g. `0..32`. Default: all shards. |
| `NOCTIS_SHARD_TOTAL` | no | Fixed shard total, a whole number of at least 1 (e.g. `16`). Default: Discord's recommendation. |
| `NOCTIS_HTTP_PROXY` | for Clusters | Shared HTTP rate-limit proxy (twilight-http-proxy, nirn-proxy) as `host:port` or `http://host:port` (plain HTTP; HTTPS is not supported). |
| `NOCTIS_GATEWAY_URL` | no | Gateway URL shards connect to instead of Discord's, as `ws://host:port` or `wss://host:port` (a gateway proxy; it must rewrite READY's `resume_gateway_url`). |
| `NOCTIS_PUBLISHER` | no | `true` (or `1`) on the one process that publishes commands globally at startup, before its shards connect. |
| `RUST_LOG` | no | Log filter, e.g. `info,noctis=debug`. |

## Run it

Publish the commands to your test guild first. Guild publishes appear instantly and only
write when something changed:

```sh
export NOCTIS_TOKEN=...
cargo run --release -- --publish-guild 123456789012345678 --dry-run   # print JSON, diff, creates
cargo run --release -- --publish-guild 123456789012345678             # write (only if changed)
cargo run --release                                                   # start the bot
```

With `NOCTIS_DEV_GUILD` set, `cargo run --release -- --publish-guild` needs no id. Each
publish mode prints the JSON, a diff (`+ create`, `~ update`, `- delete`) and the number
of creates (Discord allows 200 per day per guild), then exits without connecting.

Then try `/setup start`, `/dashboard` and `/profile` in the test guild.

Other CLI modes of the same binary:

| Flag | Effect |
| --- | --- |
| `--publish` | Publish globally (production); guild-scoped commands go only to their guilds. |
| `--publish-guild [<id>]` | Publish every command to one guild (development). |
| `--publish-clear [--guild <id>]` | Remove this App's commands globally or from a guild. |
| `--dry-run` | With a publish flag: print the JSON and the diff, write nothing. |
| `--features a,b` / `--features all` | [planned] Boot Flags: register only Modules carrying these flags. |
| `--no-resume` | [planned] Shut down without keeping sessions resumable; clears stored sessions. |

`--publish` writes only to the global target and the guilds your Registry names, so the
`--publish-guild` copy stays in your test guild (every command shows twice) and a guild
dropped from `guilds = [..]` keeps its old commands; clean such a guild with
`cargo run --release -- --publish-clear --guild <id>`.

## Develop

```sh
cargo fmt
cargo build --release
cargo clippy --release --all-targets -- -D warnings
cargo test --release
```

Always build, test, check and run with `--release`; never create a debug build (disk
space in cloud sessions is limited). If the disk fills anyway, delete `target/debug/`.

Coding agents: read `AGENTS.md`. They learn Noctis from `.claude/skills/noctis/` and
never from Noctis's source.

## Scale out later

Nothing in the handlers changes when the bot grows. Split shards across processes with
`NOCTIS_SHARDS`, point every process at one `NOCTIS_HTTP_PROXY`, and replace the
in-memory Adapters (RefStore, RateLimitStore, ModuleStateStore, IdentifyQueue,
SessionStore, CacheStore) with shared ones, as `src/adapters/` shows. See
`.claude/skills/noctis/references/app-and-sharding.md` and `adapters.md`.
