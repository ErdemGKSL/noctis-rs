---
title: 'Story 6.9: Publish with diff, dry run and CLI modes'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '2fa651644f2ba08422500432c4a2c81feeaf2830'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Registry folds every Command, but nothing writes it to Discord. A bot author cannot publish to a test guild or globally, cannot preview the change, and could burn the 200-creates-per-day quota (REG-05). The spine's built-in command line (`--publish`, `--publish-guild`, `--publish-clear`, `--dry-run`) does not exist, and `App::run()` ignores arguments. Implements FR-11 (JSON), FR-13, FR-61 (empty option list); AD-7, AD-26, AD-29, Conventions (Command line); rules META-04, REG-05.

**Approach:**
1. `noctis-core/src/publish.rs` (+ `publish/json.rs`, `publish/diff.rs`) builds the Publish JSON only from the Registry, canonicalizes local and remote command lists, diffs them and writes one bulk-overwrite `PUT` per changed target. It is reachable as `App::publish(scope)`.
2. `noctis-core/src/cli.rs` holds the built-in parser `NoctisArgs`. `App::run()` parses `std::env::args_os().skip(1)`, `App::run_with_args(args)` parses explicit arguments, and a Publish mode runs Publish, prints the result to stdout and returns without connecting shards.
3. `AppBuilder::publisher(bool)` / `NOCTIS_PUBLISHER` make a normal run publish globally once at startup, before shards connect (never on READY). `AppBuilder::dev_guild(id)` / `NOCTIS_DEV_GUILD` is the default guild for `--publish-guild` with no id.

## Boundaries & Constraints

**Always:**
- **JSON comes only from the Registry** (`Commands` scopes, `CommandNode` trees and menu entries; AD-7). One command object per base node and per menu, in Registry order.
  - **Chat input:** `{"type":1,"name","description","options":[..]}` plus each set metadata field.
    - Options of a 1-word base are its handler's `OptionDecl`s. A base with children lists them as `type` 1 (subcommand) or 2 (group, whose `options` are its subcommands).
    - Every chat-input command, subcommand and group carries `"options"`, an empty array when it has none (FR-61).
  - **Option object:**
    - Always present: `type` (`OptionType` value 3-11), `name`, `description`.
    - Only when set: `required: true`, `choices` (`[{name,value}]`, with value a string, integer or float as declared), `min_value`/`max_value` (JSON integer for `NumericValue::Int`, float otherwise), `min_length`/`max_length`, `channel_types`, `autocomplete: true`, `file_types`.
  - **Menus:** `{"type":2|3,"name"}` plus set metadata, with no `description` and no `options`.
  - **Metadata:** `default_member_permissions` is the bit string (`"8"`); `contexts` and `integration_types` are integer arrays in declared order; `nsfw` appears only when set. Unset fields are omitted, never defaulted (FR-11). Localizations are never sent (they arrive in Epic 11).
  - **Serialization:** `serde_json::to_string_pretty` of a `Value` array. Whatever key order serde_json produces is deterministic, and the tests compare parsed `Value`s, never strings.
- **Targets per scope** (`PublishScope::{Global, Guild(Id<GuildMarker>)}`):
  - `Global` (`--publish`, publisher runs): the Registry's `Scope::Global` commands go to `/applications/{app}/commands`. Then each `Scope::Guild(g)` in the Registry (first-appearance order) goes to `/applications/{app}/guilds/{g}/commands`. So guild-scoped Commands reach only their guilds (FR-11).
  - `Guild(g)` (`--publish-guild`, the dev mirror): one target, `/applications/{app}/guilds/{g}/commands`. Its body is the global commands with `contexts` and `integration_types` stripped (META-03: guild commands cannot carry them), followed by the Registry's `Scope::Guild(g)` commands.
    - Two commands with the same `(type, name)` are `PublishError::NameClash` `[REG-03]`.
    - More than 100 chat-input, 15 user or 15 message commands is `PublishError::TooManyCommands` `[REG-02]`.
  - Clear (`--publish-clear [--guild <id>]`): one target with an empty local list (global, or that guild).
- **Order of one Publish:**
  1. Open span `noctis.publish` (fields `app`, `scope`, `dry_run`, `clear`).
  2. Check the token (`PublishError::MissingToken` with no HTTP).
  3. `GET /applications/@me`, which gives the application `id` and the `integration_types_config` keys.
  4. Build every target's local JSON and run the checks: NameClash, TooMany, then META-04.
  5. For each target in order: `GET …/commands?with_localizations=true`, canonicalize, diff, then `PUT` (body = the local JSON array) only when the diff is non-empty and this is not a dry run.
  - Every check runs before the first `PUT`, so a failing check writes nothing. The first HTTP error aborts the rest.
- **META-04.** On the global target only (clear excluded), each local command's `integration_types` value must be a key of the application's `integration_types_config` (`"0"` guild install, `"1"` user install). When the field is absent or null, only `0` is enabled. A violation is `PublishError::IntegrationTypeNotEnabled { command, integration_type }`, whose Display starts `[META-04]`. The dry run reports it too.
- **Canonical form** (used for both sides; equality decides "no write"):
  - Keep only `type` (absent means 1), `name`, `description` (absent or `""` count as equal for menus), `options` (recursively; absent counts as `[]`), `default_member_permissions` (absent, `null` and unset are equal; compared as a string), `nsfw` (absent, `null` and `false` are equal), `contexts`, `integration_types`, `name_localizations`/`description_localizations` (absent, `null` and `{}` are equal).
  - An unset local `contexts` equals a remote absent, `null` or the set {0,1,2}. An unset local `integration_types` equals a remote absent, `null` or the set of enabled keys from the application object. Set lists compare as sets.
  - Option canonical fields: `required` (absent counts as false), `autocomplete` (absent counts as false), `choices`, `channel_types`, `file_types` (absent counts as `[]`), `min_value`/`max_value` (compared as f64), `min_length`/`max_length`.
  - Server fields (`id`, `application_id`, `version`, `guild_id`, `dm_permission`, `handler`) are dropped.
  - Commands match by `(type, name)`. Option and choice order matter, but command order does not.
  - Remote commands of any type other than 1, 2 or 3 (e.g. 4 Primary Entry Point) are ignored by the diff and copied verbatim into any `PUT` body for that target, so they are never deleted.
- **Diff** (`TargetReport::diff()`, one line per change, sorted by kind then name):
  - `+ create <kind> \`<name>\``
  - `~ update <kind> \`<name>\` (<changed canonical keys, comma-separated, sorted>)`
  - `- delete <kind> \`<name>\``
  - `<kind>` is `chat_input` | `user` | `message`. The diff is empty when the two sides are equal.
  - `creates()` (REG-05) is the number of `+` lines.
- **Writes are logged.** One `info!` (target `noctis::publish`, `scope`, `creates`) carries the diff before each `PUT`. When a target is unchanged, `info!` "commands unchanged; nothing written". No token and no request body appear in logs (AD-29).
- **CLI (`NoctisArgs`):**
  - Flags: `--publish`, `--publish-guild [<id>]`, `--publish-clear`, `--guild <id>` (only with `--publish-clear`) and `--dry-run`. `--flag=value` is accepted for the two value flags.
  - An id is 1-20 ASCII digits, a non-zero `u64` (leading zeros allowed).
  - For `--publish-guild`, the next token is taken as the id only when it does not start with `-`.
  - `split` moves every non-Noctis argument (argv[0], `--`, non-UTF-8, bot flags) into the rest untouched and in order. `parse` (used by `run_with_args`) turns the first such argument into `CliError::UnknownFlag`.
  - `impl IntoIterator for NoctisArgs` renders canonical flags, so `run_with_args(noctis_args)` round-trips.
- **`App::run_with_args` order:**
  1. Parse (`Error::Cli`).
  2. Runtime and crypto checks as today.
  3. With a mode: resolve `--publish-guild` with no id from `dev_guild` (else `CliError::MissingDevGuild`), run Publish, write the report to stdout with `writeln!` on `std::io::stdout().lock()` (the `print_stdout` lint), and return `Ok(())` or `Error::Publish`. No signal handler is installed, no `GET /gateway/bot` is sent and no shard starts.
  4. Without a mode: today's `run()` path. `App::run()` is `run_with_args(std::env::args_os().skip(1))`. `run_until` never parses arguments.
- **Publisher normal runs.** In `run_with`, after the token check and before `gateway_info`, when `publisher` is true: run `Global` Publish (not a dry run), awaited. An error is one `error!` (target `noctis::publish`, `%error`) and the App keeps starting. Publish is never triggered by READY or any gateway event.
- **Env.** `from_lookup` reads, after `NOCTIS_SHUTDOWN_GRACE`:
  - `NOCTIS_PUBLISHER`: ASCII-case-insensitive `true`/`false`/`1`/`0`, else `EnvError::InvalidPublisher { var }`;
  - `NOCTIS_DEV_GUILD`: the id rule above, else `EnvError::InvalidDevGuild { var }`.
  - The same trimming and empty-means-unset rules apply as for the other variables, and the error never shows the value.
- Errors are `thiserror`, `#[non_exhaustive]`. No `unwrap`/`expect` on HTTP bodies or arguments. HTTP goes through the existing raw `Request::builder(&Route::…)` pattern of `responder/transport.rs` on `AppShared.http`.

**Never:**
- No clap and no new dependency. No `--features`/`--no-resume` (Stories 13.2/12.3: they are unknown flags for now). No lifecycle `PublishCompleted` events (Story 13.5). No per-guild Module enablement filtering (Story 13.3). No localizations in JSON (Epic 11). No multi-App `apps = [..]`. No new metric.
- Never publish on shard READY. Never write when canonical sides are equal. Never send `dm_permission`.
- No debug builds. No real sleeps over 1 s in tests.

## I/O & Edge-Case Matrix

Abbreviations:
- **APP**: `GET /applications/@me` → `{"id":"7","integration_types_config":{"0":{}}}`.
- **GC/PC**: `GET`/`PUT /applications/7/commands`.
- **GG(g)/PG(g)**: the same for `/applications/7/guilds/g/commands`.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | JSON, option-less | `#[command("ping", description="Pong")]` | `{"type":1,"name":"ping","description":"Pong","options":[]}`; no metadata keys | — |
| 2 | JSON, metadata set | `permissions = MANAGE_GUILD`, `contexts=[Guild]`, `integration_types=[GuildInstall]`, `nsfw=false` | `"default_member_permissions":"32"`, `"contexts":[0]`, `"integration_types":[0]`, `"nsfw":false` | — |
| 3 | JSON, tree | `config set prefix` + `config get` + `config show` | base `config`, `options`: group `set` (type 2, options [sub `prefix` type 1 with `options`]), subs `get`/`show` type 1 | — |
| 4 | JSON, option fields | `required`, `min_value` Int(1)/Float(0.5), `max_length` 100, `choices` str/int/float, `channel_types [0]`, `autocomplete`, `file_types [".pdf"]`, unicode description `"Çalış 😀"` | each key present exactly; ints as JSON ints; unicode intact; unset keys absent; `required:false` absent | — |
| 5 | JSON, menus | `#[user_command("Report")]`, message menu | `{"type":2,"name":"Report"}`, `{"type":3,..}`, no description/options | — |
| 6 | Guild-scoped | `guilds=[42]` command + global `ping` | Global mode: PC body only `ping`; PG(42) body only the guild command | — |
| 7 | Equal remote | GC returns local JSON plus `id`, `version`, `application_id`, `dm_permission:null`, `nsfw:false`, `contexts:null`, `default_member_permissions:null`, `description_localizations:null` | requests: APP, GC; **no PUT**; diff empty; `creates()==0`; log "unchanged" | — |
| 8 | Remote defaults | unset local contexts vs remote `[0,1,2]` (any order); unset integration_types vs remote `[0]` with config `{"0":{}}` | equal, no PUT | — |
| 9 | Remote `[0,1]` contexts, local unset | | `~ update chat_input \`ping\` (contexts)`; one PUT | — |
| 10 | Differs | remote lacks `ping`, has `old`, `cfg` description differs | diff `+ create chat_input \`ping\``, `~ update chat_input \`cfg\` (description)`, `- delete chat_input \`old\``; one PC with local array; `creates()==1`; info log with diff | — |
| 11 | Numeric equality | local `min_value` Int(1), remote `1.0` | equal | — |
| 12 | Option order changed | remote options reversed | update `(options)` | — |
| 13 | Entry point preserved | remote has type 4 `launch` | not in diff; PUT body (when changed) ends with the remote type-4 object verbatim | — |
| 14 | Empty registry, empty remote | no commands | no PUT, empty diff | — |
| 15 | Dry run | `--publish --dry-run`, differs | stdout has the pretty JSON, the diff and `creates: N (REG-05: …)`; requests APP, GC only | — |
| 16 | `--publish-guild 42` | global `ping` with `contexts`, guild-42 `admin` | single target PG(42) body `[ping without contexts/integration_types, admin]`; no GC/PC; no gateway request | — |
| 17 | Dev-mirror clash | global `ping` and guild-42 `ping` | `PublishError::NameClash` `[REG-03]`, no PUT | typed |
| 18 | Dev-mirror count | 100 global + 1 guild chat-input | `TooManyCommands` `[REG-02]` count 101 max 100; 15/16 user boundary likewise | typed |
| 19 | META-04 | `integration_types=[UserInstall]`, config only `"0"`; config absent; config `{"0":{},"1":{}}` | error `[META-04]` naming command and `user_install`, no PUT (also in dry run) / error / ok | typed |
| 20 | Clear | `--publish-clear`, remote 2 commands | `- delete` ×2, PC `[]`; remote empty → no PUT; `--publish-clear --guild 42` → PG(42) | — |
| 21 | HTTP errors | APP 401; GC 500; PUT 400 `{"code":50035}`; connection refused | `PublishError::Rejected{status,code}` / `Transport`; later targets skipped | typed; `run_with_args` returns `Error::Publish` |
| 22 | Bad remote body | GC returns `{}` or non-JSON; APP lacks `id` | `PublishError::InvalidResponse` | typed |
| 23 | No token | App without token, `--publish` | `PublishError::MissingToken`, no HTTP | typed |
| 24 | Parse ok | `[]`; `--publish`; `--publish-guild 42`; `--publish-guild=42`; `--publish-guild` (no id); `--publish-guild --dry-run`; `--publish-clear --guild 42`; `--dry-run --publish`; id `007`; id `18446744073709551615` | no mode; Publish; Guild(42) ×2; Guild(None); Guild(None)+dry; Clear(42); dry Publish; 7; u64::MAX | — |
| 25 | Parse errors | `--bogus`; `positional`; `--publish --publish`; `--publish --publish-clear`; `--dry-run` alone; `--guild 1` without clear; `--publish-guild 0`; `--publish-guild 18446744073709551616`; `--publish-guild 12a`; `--publish-guild=`; `--guild` (end); `--publish=1`; `--dry-run=yes`; non-UTF-8 arg; `--publish-guild ١٢` (Arabic digits) | UnknownFlag(`--bogus`) / UnknownFlag(`positional`) / DuplicateFlag / ConflictingModes / DryRunWithoutPublish / GuildWithoutClear / InvalidGuildId ×4 (value shown) / MissingValue / UnexpectedValue ×2 / UnknownFlag(lossy) / InvalidGuildId | typed `CliError`; `run_with_args` returns `Error::Cli` before any HTTP |
| 26 | `split` | `["bot","--verbose","--publish-guild","5","x","--","--dry-run"]`; `["bot"]`; `["bot","--publish","--publish-clear"]` | mode Guild(5), dry_run false; rest `["bot","--verbose","x","--","--dry-run"]` (`--` and everything after it go to rest unparsed) / empty NoctisArgs, rest `["bot"]` / `Err(ConflictingModes)` | malformed Noctis flag → `Err(CliError)` |
| 27 | Round-trip | `NoctisArgs` → `IntoIterator` → `parse` | equal value | — |
| 28 | Missing dev guild | `--publish-guild` no id, no `dev_guild` / with `dev_guild(9)` | `CliError::MissingDevGuild` / target guild 9 | typed |
| 29 | Publisher run | `publisher(true)` normal `run_with_args([])` | APP and GC are recorded before `GET /gateway/bot`; shards start; PUT only if changed | Publish failure: one `error!` `noctis::publish`, App still reaches READY |
| 30 | Non-publisher run | default | no `/applications` request at all, before or after READY | — |
| 31 | Env | `NOCTIS_PUBLISHER` = `true`,`TRUE`,`1`,`false`,`0`,` true `,``; `yes`,`2`; `NOCTIS_DEV_GUILD` `42`,`0`,`-1`,`4x`, 21 digits | true,true,true,false,false,true,unset; `InvalidPublisher`; 42 / `InvalidDevGuild` ×4 (message has no value) | typed `EnvError` |
| 32 | Span | any Publish | one `noctis.publish` span with `app`,`scope`,`dry_run` | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/registry/commands.rs`:
  - `Commands` (`scope(Scope)`, private `scopes` Vec in first-appearance order; add `pub(crate) fn scopes(&self) -> &[ScopeCommands]`);
  - `ScopeCommands { scope, bases: Vec<CommandNode>, menu_order }`;
  - `CommandNode { word, description, metadata, handler, children }`.
  - `base_metadata` gives the resolved base metadata. Menus: `entry.handler.kind()/menu_name()/metadata()`.
- `crates/noctis-core/src/command.rs` L486-530: `CommandHandler::{options() -> &'static [OptionDecl], description(), metadata()}`.
- `crates/noctis-rules/src/command/option.rs` L180-420: `OptionDecl`, `OptionType` (add `pub const fn value(self) -> u8` if missing), `NumericValue::{Int,Float}`, `Choice`, `ChoiceValue`. `meta.rs`: `InteractionContext::value()/name()`, `IntegrationType::value()/name()`. `mod.rs` L256: `Scope::{Global, Guild(u64)}`.
- `crates/noctis-core/src/metadata.rs`: `CommandMetadata` (public fields; `default_member_permissions: Option<Permissions>`).
- `crates/noctis-core/src/responder/transport.rs` L336-380: the `send()` raw-request pattern and `http_error`/`error_code`. Copy the pattern into `publish.rs` (it maps to `PublishError`). twilight routes: `Route::GetCurrentUserApplicationInfo`, `GetGlobalCommands { application_id, with_localizations: Some(true) }`, `SetGlobalCommands`, `GetGuildCommands`, `SetGuildCommands` (`twilight-http` 0.17.1 `routing.rs` L500-1004).
- `crates/noctis-core/src/app.rs`:
  - `App` struct L103 (add `publisher: bool`, `dev_guild: Option<Id<GuildMarker>>`);
  - `run` L424 and `run_with` L500 (insert the publisher Publish after the token check, before `warm_up`/`gateway_info`);
  - `AppBuilder` L1012, setters pattern L1135-1300;
  - `from_lookup` L1096, `read_var` L1756, `parse_shard_total` (id parse pattern).
- `crates/noctis-core/src/error.rs`:
  - `Error` L26: add `Cli(#[from] CliError)` and `Publish(#[from] PublishError)`.
  - `EnvError` ~L485: add `InvalidPublisher { var }` and `InvalidDevGuild { var }`.
- `crates/noctis-core/src/lib.rs`: module declarations and re-exports, plus crate docs.
- `crates/noctis/src/lib.rs`:
  - L434 re-exports;
  - add `pub mod cli { pub use noctis_core::cli::{NoctisArgs, CliMode, CliError}; }` and `pub mod publish { pub use noctis_core::publish::{Publish, PublishScope, PublishReport, TargetReport, PublishError}; }` with module docs.
- Tests:
  - `crates/noctis-testkit/src/fake.rs`: `FakeDiscordHttp::{start, start_with, route, route_with, requests, url}`; `RecordedRequest::{route_path, json, query}`.
  - `crates/noctis-testkit/tests/commands.rs` L130-200 is the harness pattern (token, `http_proxy(http.url())`, `gateway_url`).
- `App::run()` callers that must switch to `run_with_args(std::iter::empty::<&str>())`, because the test harness passes its own argv:
  - `crates/noctis-core/src/app/tests.rs` L501, 513, 528, 533, 1182, 1201;
  - `crates/noctis-core/tests/crypto_preinstalled.rs` L21;
  - `crates/noctis-testkit/tests/{commands.rs:182, events.rs:196, lane_metrics.rs:92, observability.rs:350, sharding.rs:110, sharding_warning.rs:69, shutdown_signal.rs:38}`.
  - `no_run` doctests stay as they are.
- `xtask/rules-pending.toml` L41-49: remove the META-04 and REG-05 entries (they need `unit` tests whose names contain `meta_04` / `reg_05`).
- `docs/discord-rules.md` L138 (META-04) and L149 (REG-05) How cells; L146 REG-02 (dev-mirror recount).
- Template:
  - `examples/project-template/`: `src/main.rs` L27-36 comment, `README.md` L60-100, `AGENTS.md`;
  - `.claude/skills/noctis/`: `SKILL.md` (API hash, L301, L318), `references/app-and-sharding.md` (L121, L263, L269, §3 L275-306, L585 span), `references/commands.md` §7 L744, `references/troubleshooting.md`.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/cli.rs` (new, `pub mod cli`):
   - `#[non_exhaustive] pub enum CliMode { Publish, PublishGuild(Option<Id<GuildMarker>>), PublishClear(Option<Id<GuildMarker>>) }`.
   - `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct NoctisArgs { mode: Option<CliMode>, dry_run: bool }` with:
     - `parse<I: IntoIterator<Item = A>, A: Into<OsString>>(args) -> Result<Self, CliError>`;
     - `split(args) -> Result<(Self, Vec<OsString>), CliError>`;
     - `mode()`, `dry_run()`, and `IntoIterator<Item = OsString>`.
   - `#[non_exhaustive] pub enum CliError { UnknownFlag{flag}, MissingValue{flag}, UnexpectedValue{flag}, InvalidGuildId{flag,value}, DuplicateFlag{flag}, ConflictingModes{first,second}, DryRunWithoutPublish, GuildWithoutClear, MissingDevGuild }`. Each Display names the flag and gives a fix.
   - Unit tests for Matrix 24-27, with doctests on the public items.
2. `crates/noctis-core/src/publish.rs` + `publish/{json.rs,diff.rs,tests.rs}` (`pub mod publish`):
   - Public: `PublishScope` (Copy, Eq, Display `global`/`guild <id>`); `Publish<'a>` with `dry_run(bool)`, `clear(bool)` and `IntoFuture<Output = Result<PublishReport, PublishError>>` (boxed `Send` future); `PublishReport` (`targets()`, `creates()`, `wrote()`, `is_dry_run()`, Display = the dry-run printout); `TargetReport` (`scope()`, `json()`, `diff()`, `creates()`, `changed()`, `wrote()`); `#[non_exhaustive] PublishError { MissingToken, Rejected{route,status,code}, Transport{route}, InvalidResponse{route,reason}, IntegrationTypeNotEnabled{command,integration_type}, NameClash{guild,kind,name}, TooManyCommands{guild,kind,count,max} }`. `route` is the method plus path. META-04, REG-03 and REG-02 messages start with their `[ID]`.
   - Private: `json::local(scope_commands) -> Vec<Value>`, `diff::canonical(&Value, &Defaults) -> Option<(Key, Value)>`, `diff::diff(local, remote) -> Diff`.
   - Unit tests for Matrix 1-5 and 7-14 (pure functions), including `meta_04_rejects_disabled_integration_type` and `reg_05_counts_creates`.
3. `crates/noctis-core/src/app.rs` + `error.rs`:
   - `App::publish(&self, PublishScope) -> Publish<'_>`, `App::run_with_args`, `App::is_publisher()`, `App::dev_guild()`;
   - `AppBuilder::publisher(bool)`, `AppBuilder::dev_guild(Id<GuildMarker>)` (the last call wins), the env reads, the publisher hook in `run_with`, and the `Error`/`EnvError` variants.
   - Switch the listed `run()` test callers. Add env tests (Matrix 31) in `app/tests.rs`.
4. `crates/noctis-testkit/tests/publish.rs` (new), end to end with `FakeDiscordHttp` (a `route_with` closure keeps remote state so a PUT is visible to the next GET):
   - Matrix 6, 7, 10, 15, 16, 17, 19, 20, 21, 23, 25 (via `run_with_args`), 28, 29, 30;
   - `publish_never_runs_on_shard_ready` (30);
   - `meta_04_blocks_publish_before_put`;
   - `reg_05_dry_run_reports_creates`.
   - Span (32) with the observability capture pattern.
5. `crates/noctis-core/src/lib.rs` + `crates/noctis/src/lib.rs`:
   - Re-exports, the `cli`/`publish` facade modules, and a crate-doc "Publishing commands" section (`no_run` example using `app.publish(PublishScope::Guild(id)).dry_run(true).await?`).
   - Run `cargo run --release -p xtask -- api-snapshot --bless`.
6. `xtask/rules-pending.toml`: delete the META-04 and REG-05 entries.
   - `docs/discord-rules.md` META-04: "`App::publish` reads `GET /applications/@me` and returns `PublishError::IntegrationTypeNotEnabled` before any PUT". REG-05: diff, `creates()`, dry-run line. REG-02: add "the `--publish-guild` mirror re-counts (`TooManyCommands`)". Keep the Enforcement cells.
7. `examples/project-template/`:
   - Usage Skill: remove `[planned, Story 6.9]` from `.publisher`, `NOCTIS_DEV_GUILD`, `NOCTIS_PUBLISHER` and the `noctis.publish` span;
   - rewrite §3: the real flags, `split` returning `Result` (`let (noctis_args, mine) = NoctisArgs::split(std::env::args_os())?;`), the `PublishReport` API, the diff format, the dev mirror stripping contexts/integration_types, publisher behaviour, and `--features`/`--no-resume` marked `[planned]`;
   - `commands.md` §7;
   - troubleshooting rows (META-04, REG-03 clash, `UnknownFlag` under `cargo test` → use `run_with_args`);
   - `SKILL.md` API hash;
   - `main.rs` comment: drop `[planned]` for the publish flags;
   - README run section (`cargo run --release -- --publish-guild <id>`);
   - one `AGENTS.md` bullet.

**Acceptance Criteria:**
- Given the Registry, when Publish builds JSON, then it comes only from the Registry, set metadata appears exactly as declared, unset fields are absent, option-less commands carry `"options":[]`, and guild-scoped commands go only to their guilds.
- Given remote equal to local in `FakeDiscordHttp`, when `app.publish(PublishScope::Global).await` runs, then no PUT is recorded. When they differ, exactly one PUT per changed target is recorded and the diff is logged at info.
- Given `run_with_args(["--publish", "--dry-run"])`, when it runs, then stdout gets the JSON, the diff and the create count, no PUT is recorded, it returns `Ok(())`, and no `/gateway/bot` request is made.
- Given each of `--publish`, `--publish-guild <id>` and `--publish-clear`, when `run_with_args` sees it, then it performs that mode and returns without connecting shards. A normal run publishes only with `publisher(true)` and never on READY.
- Given an unknown flag, when it reaches `run_with_args`, then `Error::Cli(CliError::UnknownFlag { flag })` names it. `NoctisArgs::split` returns the remaining arguments untouched.
- Given a declared `integration_types` not enabled for the application, when Publish runs (dry run included), then it fails with `[META-04]` before any PUT, inside a `noctis.publish` span.
- Given the new API, when the gates run, then the Usage Skill and the template README document the publish workflow (`cargo run --release -- --publish-guild <id>`), and `api-snapshot`, `skill-check` and `rules-catalog` pass (META-04 and REG-05 covered, none pending).

## Spec Change Log

## Review Triage Log

## Design Notes

**Canonical diff (decision).** Discord's GET fills in defaults (`nsfw:false`, `contexts`, `integration_types`, null localizations) and server fields. Comparing raw JSON would rewrite on every Publish. The canonical form applies Discord's documented defaults to unset local fields. An unset local `contexts` matches `[0,1,2]` (Discord's default for new commands). An unset `integration_types` matches the app's enabled install types. A spurious update never costs creates (REG-05 counts only new names), but a missed change would be a bug, so every other field is compared strictly.

**Dev mirror (decision).** `--publish-guild` publishes the global set plus that guild's own commands into one guild, so development sees everything instantly. `contexts`/`integration_types` are stripped there because Discord rejects them on guild commands (META-03). Name clashes and counts are re-checked, because the union was never folded by `validate_merge`.

**Test harness argv (decision).** `App::run()` must read `std::env::args()` (spine), but `cargo test` passes harness flags. So every test that starts an App switches to `run_with_args(empty)`, and the troubleshooting doc says so.

**Publisher timing (decision).** The publisher publishes before shards connect, and failure is logged, not fatal: a Discord hiccup must not stop the bot from serving. The CLI modes return the error.

**`--publish-guild` optional id and `--publish-clear --guild` (decision).** The Usage Skill and README already promise them. `NOCTIS_DEV_GUILD` feeds `AppBuilder::dev_guild`.

```rust
let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
assert_eq!(report.creates(), 1);
println!("{report}");
```

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, including `noctis-testkit --test publish` and the `cli`/`publish` unit tests
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok, META-04 and REG-05 covered
- `cargo run --release -p xtask -- api-snapshot --bless` then `cargo run --release -p xtask -- api-snapshot` -- expected: ok; hash copied to SKILL.md (needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok
