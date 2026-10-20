# Noctis bot template

A ready-to-copy Discord bot built on [Noctis](https://crates.io/crates/noctis): a Rust
framework on twilight-rs with built-in sharding, a Components V2 UI kit and Tailwind
image rendering.

> **Status:** written against the planned Noctis API. It will compile once the `noctis`
> crate lands; until then treat it as the reference shape of a Noctis bot.

## What's inside

| Feature | Where | Shows |
| --- | --- | --- |
| `/setup start` | `src/modules/setup.rs` | Wizard with Steps, validation, selects, Finish |
| `/dashboard` | `src/modules/dashboard.rs` | Navbar tabs, auto-paginated role list, module toggles |
| `/profile`, "Show profile" | `src/modules/profile.rs` | Rendered card (Template) in a Media Gallery, bio modal with pre-fill, XP event handler, user command |
| Custom Adapter | `src/adapters/bounded_cache.rs` | Implementing the `CacheStore` port yourself |
| Locales | `locales/en-US.yaml`, `locales/tr.yaml` | Typed strings, word-by-word command translation |
| Tests | `tests/` | Testkit flows, Adapter conformance, pixel snapshot |
| Agent setup | `AGENTS.md`, `.claude/skills/` | Noctis Usage Skill + rust-best-practices |

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
variables read by `App::from_env()`. Copy `.env.example` to `.env` and export it, or set
the variables in your deployment.

| Variable | Required | Meaning |
| --- | --- | --- |
| `NOCTIS_TOKEN` | yes | Bot token. Never logged. |
| `NOCTIS_PAYLOAD_KEY` | for `signed` Routes and Clusters | 32-byte key (base64) for `custom_id` integrity tags. Same value on every process. `openssl rand -base64 32`. |
| `NOCTIS_PAYLOAD_KEY_PREVIOUS` | no | Previous key, accepted for verification while you rotate. |
| `NOCTIS_APP_ID` | no | Key namespace for Adapters (`[a-z0-9-]{1,32}`); `configure` sets `noctis-template`. |
| `NOCTIS_DEV_GUILD` | no | Default guild for `--publish-guild` with no id. |
| `NOCTIS_SHARDS` | no | Shard Range for this process, e.g. `0..32`. Default: all shards. |
| `NOCTIS_SHARD_TOTAL` | no | Fixed shard total. Default: Discord's recommendation. |
| `NOCTIS_HTTP_PROXY` | for Clusters | Shared HTTP rate-limit proxy (twilight-http-proxy, nirn-proxy) as `host:port` or `http://host:port` (plain HTTP; HTTPS is not supported). |
| `NOCTIS_PUBLISHER` | no | `true` on the one process that publishes commands at startup. |
| `RUST_LOG` | no | Log filter, e.g. `info,noctis=debug`. |

## Run it

Publish the commands to your test guild first. Guild publishes appear instantly and only
write when something changed:

```sh
export NOCTIS_TOKEN=...
cargo run --release -- --publish-guild 123456789012345678 --dry-run   # print JSON and the diff
cargo run --release -- --publish-guild 123456789012345678             # write
cargo run --release                                                   # start the bot
```

Then try `/setup start`, `/dashboard` and `/profile` in the test guild.

Other CLI modes of the same binary:

| Flag | Effect |
| --- | --- |
| `--publish` | Publish globally (production). |
| `--publish-guild <id>` | Publish to one guild (development). |
| `--publish-clear [--guild <id>]` | Remove this App's commands globally or from a guild. |
| `--dry-run` | With a publish flag: print the JSON and the diff, write nothing. |
| `--features a,b` / `--features all` | Boot Flags: register only Modules carrying these flags. |
| `--no-resume` | Shut down without keeping sessions resumable; clears stored sessions. |

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
