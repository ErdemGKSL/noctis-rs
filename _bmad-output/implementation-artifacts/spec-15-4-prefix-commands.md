---
title: 'Story 15.4: Prefix commands'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '10675ec451c684adbc8bf3183eb3a99e2066b0c0'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The template's new `prefix` feature path (`examples/project-template/src/lib.rs` `#[cfg(feature = "prefix")]` block, `prefix` feature, `Profile`/`Badge` re-exports) is compiled by no gate.
    evidence: |-
      The template is excluded from the workspace, `cargo hack --each-feature` does not reach it, and the disabled `template-build` CI job is tracking-only (`continue-on-error: true`) and builds default features only. A triage check with the workspace lock showed the template already fails to build for pre-existing reasons (planned `rate_limit`, remote `#[asset]`, `noctis::id`), so the prefix block cannot be verified until Story 14.2 makes `template-build` blocking; that job should then also build with `--features prefix`.
    location: >-
      examples/project-template/src/lib.rs:65-76
    severity: medium
---

<intent-contract>

## Intent

**Problem:** A bot migrating from dbi cannot let users type `!command args`: `noctis-prefix` is an empty crate, the facade's `prefix` feature turns nothing on, and the existing `Dispatcher::run_command` (Story 6.5) has no caller that parses messages, so FR-14 is unmet.

**Approach:** Implement `PrefixPlugin` in `noctis-prefix` (behind the facade's `prefix` feature, off by default): authors opt slash Commands in by type (`PrefixPlugin::new("!").command::<Warn>()`); `install` validates the config, mounts a `MESSAGE_CREATE` Event Handler that needs `MESSAGE_CONTENT` (GW-05 via the existing intent fold), and the handler parses prefix, Command Path and positional arguments against the Command's `OptionDecl`s, then calls `ctx.dispatcher().run_command(..)`. Core gains only generic, prefix-agnostic seams: `CommandInput::with_error`/`error`/`with_resolved`, `OptionError::Constraint`, a hidden extra-intent hook on `EventHandler`, and a hidden `__rules` re-export.

## Boundaries & Constraints

**Always:**
- Core contains no prefix parsing code; `noctis-prefix` depends on `noctis-core` only among internal crates (deps-check `undrawn-edge`), reaching `noctis_rules` types through a new `#[doc(hidden)] pub use noctis_rules as __rules;` in `noctis-core`.
- Feature off: the facade does not depend on `noctis-prefix` (optional dep, `prefix = ["dep:noctis-prefix"]`), `noctis::prefix` does not exist, and no intent changes.
- Every argument failure is an `OptionError` carried in the `CommandInput` via `with_error`, so the run goes through `Dispatcher::run_command` and the pipeline answers exactly like a failed slash decode (FR-10): `noctis.validation.failed` notice (public reply), Command Error Sink report, handler not run. The first failure in declaration order wins.
- Command Path words match case-insensitively (`str::to_lowercase` of the typed word equals the path word); the prefix matches exactly (case-sensitive) and must be followed directly by the first path word.
- Ignored silently (no request, no log above `debug`): author is a bot (`author.bot`), message has `webhook_id`, message kind is not `Regular`/`Reply`, content does not start with the prefix, no opted-in path matches, the Command's `contexts` exclude the place (`Guild` needed when `guild_id` is Some, `BotDm` when None; `contexts: None` allows both), `DispatchError::UnknownCommand`.
- A non-matching message does no heap allocation in the plugin (prefix check on `&str` first).
- Follow `rust-best-practices`; release-only cargo; public API change updates template, its `AGENTS.md`, the Usage Skill and `xtask/public-api.txt` + `API hash` in the same commit.

**Never:**
- No per-guild/dynamic prefix, no mention-as-prefix, no named (`name:value`) arguments, no `MESSAGE_UPDATE` (edited messages) handling, no help command, no `#[command(.., prefix)]` macro argument, no change to `noctis-macros`.
- No permission computation: Commands whose `default_member_permissions` is `Some(_)`, whose `nsfw` is `Some(true)`, or that have a CHANNEL option with non-empty `channel_types` cannot be opted in (install violation), because a message carries no data to enforce them.
- No HTTP/cache lookups to resolve ids; `members` in the resolved data stay empty (a message has no member permissions).
- Never add `MESSAGE_CONTENT` automatically; never answer bots.

## I/O & Edge-Case Matrix

Setup unless noted: prefix `!`; opted-in `Warn` at path `warn` with options `user: USER (required)`, `days: INTEGER (optional, min 1, max 30)`, `reason: STRING (optional, max_length 10)`; `Say` at `say` with `text: STRING (required)`; `Cfg` at `config set prefix` with `value: STRING (required)`; `Flag` at `flag` with `on: BOOLEAN (required)`, `ratio: NUMBER (optional)`; `Pick` at `pick` with `color: STRING (required, choices red/green)`; `File` at `file` with `doc: ATTACHMENT (required, file_types [".pdf"])`, `note: STRING (optional)`; `Ping` (unit struct) at `ping`; `Role` at `role` with `r: ROLE`, `m: MENTIONABLE (optional)`. "Err X" = handler not run, input carries that `OptionError`, public validation notice replied, Command Error Sink gets it.

| # | Scenario | Input / State | Expected Output / Behavior |
|---|---|---|---|
| R1 | Happy path, mention | `!warn <@5> 3 rude` (5 in `mentions`) | `user=User(5)` with user 5 in `resolved.users`, `days=Integer(3)`, `reason=String("rude")`; same handler runs; reply to message |
| R2 | Nickname mention | `!warn <@!5>` | `user=User(5)`; optional ones absent |
| R3 | Raw id | `!warn 5` (5 not mentioned, not author) | `User(5)`, not in resolved (an `Id<UserMarker>` field works; `ResolvedUser` → Err `Unresolved` from decode) |
| R4 | Raw author id | `!warn <author id>` | author's `User` put in `resolved.users` |
| R5 | Rest-of-line | `!say hello  big   world` | `text=String("hello  big   world")` (verbatim after trimming outer whitespace) |
| R6 | Rest-of-line, one quoted token | `!say "hi there"` | `text="hi there"` |
| R7 | Rest-of-line, quotes inside | `!say he said "hi"` | `text=he said "hi"` verbatim |
| R8 | Quoted mid token | `!warn 5 3 "a b"` | `reason="a b"` (last STRING; whole remainder is one quoted token) |
| R9 | Escapes | `!say "a \"q\" \\ b"` | `a "q" \ b`; other backslashes literal |
| R10 | Unclosed quote | `!warn 5 "3` | Err `Malformed{reason contains "unclosed quote"}` |
| R11 | Junk after closing quote | `!flag "true"x` | Err `Malformed{.. "after closing quote"}` |
| R12 | Extra argument, last not STRING | `!flag true 1.5 extra` | Err `Malformed{.. "unexpected argument `extra`"}` |
| R13 | Missing required | `!warn` | no option set → Err `Missing{option:"user"}` (from decode) |
| R14 | Integer not numeric | `!warn 5 abc` | Err `WrongType{option:"days", expected:Integer, found:String}` |
| R15 | Integer looks fractional | `!warn 5 1.5` | Err `WrongType{expected:Integer, found:Number}` |
| R16 | i64 overflow | `!warn 5 99999999999999999999` | Err `OutOfRange{option:"days", value:"99999999999999999999"}` |
| R17 | Beyond ±(2^53−1), in i64 | INTEGER without bounds, `9007199254740992` | Err `Constraint{option, reason starts "[OPT-03]"}` |
| R18 | Below / above min/max | `!warn 5 0`, `!warn 5 31` | Err `Constraint{reason starts "[OPT-05]"}`; `1` and `30` accepted |
| R19 | Signs | `+5`, `-0` | `Integer(5)`, `Integer(0)` (then bounds apply) |
| R20 | Non-ASCII digits / hex / underscores | `١٢`, `0x10`, `1_000` | Err `WrongType{found:String}` |
| R21 | Number forms | `!flag true 1e3`, `.5`, `-2.` | `Number(1000.0)`, `0.5`, `-2.0` |
| R22 | Number words / overflow | `nan`, `inf` → Err `WrongType{Number, String}`; `1e400` or `9007199254740994` (> 2^53) → Err `Constraint{"[OPT-04]"}` |
| R23 | Boolean | `true/false/yes/no/on/off` any ASCII case → `Boolean`; `1`, `maybe` → Err `WrongType{Boolean, String}` |
| R24 | STRING length | `!warn 5 3 12345678901` (11 chars, max 10) | Err `Constraint{"[OPT-06]"}`; length is `noctis_rules::len::discord_len`; 10 chars accepted; `"🙂"×10` counts per `discord_len` |
| R25 | Choices by value / name | `!pick red`, `!pick Green` (name `Green`, value `green`) | `String("red")`, `String("green")` (value exact, else name ASCII-case-insensitive) |
| R26 | Unknown choice | `!pick blue` | Err `UnknownChoice{option:"color", value:"blue"}` |
| R27 | Attachment | `!file` with 1 attachment `a.PDF` | `doc=Attachment(id)`, attachment in `resolved.attachments`; attachments bind in order to ATTACHMENT options, consuming no text |
| R28 | Attachment filter | `!file` with `a.png` | Err `Constraint{"[OPT-13]"}` (dot entries: case-insensitive filename suffix; `image`/`video`/`audio`: `content_type` prefix `image/` …) |
| R29 | Attachment missing | `!file` no attachments | Err `Missing{option:"doc"}`; `note` is the rest-of-line STRING (last non-ATTACHMENT option) |
| R30 | Role / mentionable | `!role <@&7> <@&8>`; `!role 7 <@9>` | `Role(7)`, `Mentionable(8)`; `Mentionable(9)` with user 9 resolved if mentioned |
| R31 | Bad ids | `<@0>`, `<@abc>`, `@everyone`, 21-digit id, `<@&7>` for USER | Err `WrongType{expected:User/Role/..., found:String}` |
| R32 | Unit command with args | `!ping x` | Err `Malformed{"unexpected argument `x`"}`; `!ping` runs |
| R33 | Multi-word path, spacing | `!config  set\u{3000}prefix ?` | path `config set prefix`, `value="?"` (any `char::is_whitespace` run separates path words) |
| R34 | Case | `!WARN <@5>`, `!ÜNLÜ` for path `ünlü` | matches (lowercased) |
| R35 | Partial / glued path | `!config set` (only `config set prefix` opted in), `!warn5`, `!config` | ignored (no request) |
| R36 | Prefix edge | `!`, `! warn 5`, `?warn 5`, `!!warn` | ignored |
| R37 | Unicode / multi-char prefix | prefix `ş>` with `ş>warn <@5>` | runs |
| R38 | Bot / webhook / system | `author.bot`, `webhook_id` set, kind `GuildMemberJoin` | ignored |
| R39 | Contexts | `contexts=[Guild]` invoked in DM; `[BotDm]` in guild | ignored |
| R40 | Not mounted / other guild scope | opted in but `Dispatcher` returns `UnknownCommand` | ignored, `debug` log |
| P1 | Prefix empty | `PrefixPlugin::new("")` | `BuildViolation::plugin::<PrefixPlugin>` "prefix is empty" |
| P2 | Prefix whitespace | `"! "`, `"a b"` | violation "prefix contains whitespace" |
| P3 | Prefix length | 32 chars ok; 33 chars (Unicode scalars) | violation "prefix is longer than 32 characters" |
| P4 | No commands | `PrefixPlugin::new("!")` alone | violation "no Command is opted in: add `.command::<T>()`" |
| P5 | Unenforceable metadata | `default_member_permissions` Some / `nsfw = true` / CHANNEL with `channel_types` | one violation each, naming the struct (`__Kind::NAME`) and the reason |
| P6 | Path clash | two different types with a shared path | violation naming the path; same type twice is deduplicated silently |
| P7 | GW-05 | plugin installed, `MESSAGE_CONTENT` not requested | `BuildViolation::MissingIntent{module:"noctis-prefix", event:"MESSAGE_CREATE", intent: MESSAGE_CONTENT}` |
| P8 | Intents ok | `.intents(Intents::MESSAGE_CONTENT)` | builds; `app.intents()` contains `GUILD_MESSAGES | MESSAGE_CONTENT` |

</intent-contract>

## Code Map

- `crates/noctis-core/src/pipeline.rs` L1366-1655 -- `Dispatcher`, `CommandInvocation` (builder: `new(input, channel, message, user)`, `.guild_id`, `.shard`, `.locale`), `run_command` (resolves guild scope then global; `DispatchError::UnknownCommand`; message-reply Responder; public notices). Reuse as is; update its doc to name `PrefixPlugin` as the shipped caller.
- `crates/noctis-core/src/options.rs` -- `OptionValue` (L117), `OptionError` (L171, `#[non_exhaustive]`), `CommandInput` (L295; `new`, `with_option`, `get`, `resolved`; `empty_resolved()`). Add `error: Option<OptionError>` field + `with_error`, `error()`, `with_resolved`; add `OptionError::Constraint`.
- `crates/noctis-core/src/command.rs` L725 `CommandHandler::call` -- single choke point for all four handler shapes: return `failed(HandlerError::new(err.clone()))` when `ctx.input().error()` is Some, before the shape closure (covers unit commands, R32). `decode<O>` L751 unchanged.
- `crates/noctis-core/src/handler.rs` L416-490 `EventHandler` (`__new`, `intent`) -- add hidden `pub fn __with_intents(self, extra: Intents) -> Self` (ORs into `intent`).
- `crates/noctis-core/src/registry.rs` L26 `PRIVILEGED` (already has `MESSAGE_CONTENT`), L205-222 fold -- split a handler's intent: `privileged = intent & PRIVILEGED` must be ⊆ `requested` (violation carries only the missing privileged bits), non-privileged part always added. Existing single-intent behaviour unchanged.
- `crates/noctis-core/src/error.rs` L402 `MissingIntent`, L877 `IntentName` (prints `MESSAGE_CONTENT` already).
- `crates/noctis-core/src/kind.rs` L111 `__SlashCommand` (`PATTERN`, `PATHS`, `METADATA`, `EPHEMERALITY`) + `Options::OPTIONS`, `__Kind::NAME` -- what `PrefixPlugin::command::<T>()` reads.
- `crates/noctis-core/src/metadata.rs` L35 `CommandMetadata` (`default_member_permissions`, `contexts`, `nsfw`).
- `crates/noctis-core/src/plugin.rs`, `app.rs` L2349 `plugin`, L2400 `report`, L2155 `module`, L1874 `intents`; `BuildViolation::plugin::<P>(msg)`.
- `crates/noctis-core/src/module.rs` -- `Module::new(id).__add(EventHandler)` (hidden, public) is how the plugin mounts its handler.
- `crates/noctis-core/src/events.rs` -- `MessageCreate` (twilight `Message`: `author`, `content`, `guild_id`, `channel_id`, `id`, `mentions: Vec<Mention>`, `attachments`, `webhook_id`, `kind`).
- `crates/noctis-core/src/lib.rs` L312-313 re-exports; add `#[doc(hidden)] pub use noctis_rules as __rules;`.
- `crates/noctis-prefix/{Cargo.toml,src/lib.rs}` -- empty crate today; deps allowed: `noctis-core` (internal) + external `twilight-model`, `tracing` (workspace versions).
- `crates/noctis/Cargo.toml` L29-30 `prefix = []`; `crates/noctis/src/lib.rs` L586 feature doc, L2353 `__private`, image module gating pattern L2025.
- `xtask/src/deps_check.rs` L103 (prefix→core drawn; facade→prefix allowed when gated by the `prefix` feature); `xtask/src/api_snapshot.rs` L68 `HIDDEN` (add `noctis_core::__SlashCommand`/`__Kind` → `noctis::__private::core::…` if the snapshot names them).
- `crates/noctis/tests/render_e2e.rs` -- pattern for a feature-gated facade e2e with `TestApp`; `noctis_testkit::TestApp::{build, gateway_fake, http_fake}`, `FakeGateway::dispatch(event_type, json)`, `FakeDiscordHttp::requests()`.
- `crates/noctis-core/src/pipeline/tests.rs` L723 `dispatcher_runs_commands`, L822 `event_handler_dispatches`, L2743 `dispatcher_notices_are_public` -- patterns for core tests.
- `docs/discord-rules.md` L296 GW-05 row; no rules-pending entry exists for 15.4.
- Skill: `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` L965 (`PrefixPlugin` `[planned]` bullet), `references/responder.md` §4.1 L597-640 (Dispatcher), `references/commands.md`, `references/troubleshooting.md`, `SKILL.md` L23 API hash and L485 index; `examples/project-template/{Cargo.toml,src/lib.rs,AGENTS.md}`; template commands `Profile` (`user: Option<ResolvedUser>`), `Badge` (`name: String`) in `src/modules/profile/handlers.rs`.
- `.github/workflows-disabled/ci.yml` L65-67 feature-specific test lines.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/options.rs` -- `OptionError::Constraint { option: String, reason: String }` (Display `option `{option}`: {reason}`; reason starts with the rule id, e.g. `[OPT-05] 0 is below min_value 1`); `CommandInput::with_error(self, OptionError) -> Self` (keeps the first error), `error(&self) -> Option<&OptionError>`, `with_resolved(self, InteractionDataResolved) -> Self`; doctests; unit tests.
- `crates/noctis-core/src/command.rs` -- short-circuit in `CommandHandler::call` on `input.error()`.
- `crates/noctis-core/src/handler.rs`, `registry.rs` -- `EventHandler::__with_intents`; fold split of privileged/non-privileged bits; registry test `gw_05_mixed_intent_handler` (missing `MESSAGE_CONTENT` → violation with only that bit; `GUILD_MESSAGES` still derived).
- `crates/noctis-core/src/lib.rs` -- hidden `__rules` re-export; `pipeline.rs` Dispatcher doc names `PrefixPlugin`.
- `crates/noctis-core/src/pipeline/tests.rs` -- dispatcher test: input `with_error(WrongType{..})` → handler not run, public `noctis.validation.failed` reply, Command Error Sink receives a report whose error downcasts to `OptionError`; unit command same.
- `crates/noctis-prefix/Cargo.toml` -- deps `noctis-core`, `twilight-model`, `tracing` (all `.workspace = true`); dev-deps none beyond tokio if needed.
- `crates/noctis-prefix/src/lib.rs` -- crate docs; `pub struct PrefixPlugin` (`new(impl Into<String>)`, `command::<T: __SlashCommand>(self) -> Self`, `Debug`); `impl Plugin` (`install`: run P1-P6 checks with `app.report(BuildViolation::plugin::<PrefixPlugin>(..))`, then `app.module(Module::new("noctis-prefix").__add(EventHandler::__new::<MessageCreate, _, _>("noctis_prefix::PrefixPlugin", false, false, handler).__with_intents(Intents::MESSAGE_CONTENT)))`, closure capturing an `Arc` table).
- `crates/noctis-prefix/src/parse.rs` -- pure functions: `match_path(content, prefix, table) -> Option<(entry, args_str)>`, tokenizer, `bind(args, decls, &Message) -> CommandInput` implementing R1-R37; exhaustive unit tests for every matrix row R1-R37 and P1-P6 (test fns over the pure functions; build `OptionDecl`s with `noctis_core::__rules::command::OptionDecl::new(..)` and `Message` from JSON via `serde_json` dev-dep if needed).
- `crates/noctis/Cargo.toml` -- `noctis-prefix = { workspace = true, optional = true }`, `prefix = ["dep:noctis-prefix"]`.
- `crates/noctis/src/lib.rs` -- `#[cfg(feature = "prefix")] pub mod prefix { pub use noctis_prefix::PrefixPlugin; }` with module docs (opt-in, MESSAGE_CONTENT, argument grammar, limitations, `no_run` example building an App); feature list line updated.
- `crates/noctis/tests/prefix_e2e.rs` (new, `#![cfg(all(feature = "prefix", feature = "testkit"))]`) -- `TestApp` with `Warn`-like command + plugin + `MESSAGE_CONTENT`: R1 (POST `/channels/{c}/messages` replying to the message, handler saw decoded options), R14 (notice reply + sink got `OptionError::WrongType`), R38 bot ignored (no request), P7 `gw_05_prefix_needs_message_content` (`TestError::Build` with `MissingIntent` for `MESSAGE_CONTENT`), P8.
- `docs/discord-rules.md` -- GW-05 How: add "`PrefixPlugin` (`prefix` feature) mounts a `MESSAGE_CREATE` handler that needs `MESSAGE_CONTENT`; without `.intents(Intents::MESSAGE_CONTENT)` build fails with `MissingIntent`". No `noctis-rules` change (no limit changed).
- `.github/workflows-disabled/ci.yml` -- add `cargo test --release -p noctis --features prefix,testkit --test prefix_e2e` next to the `render_e2e` line.
- `examples/project-template/Cargo.toml` -- `[features] prefix = ["noctis/prefix"]` (off); `src/lib.rs` `configure` -- `#[cfg(feature = "prefix")]` block adding `.intents(Intents::MESSAGE_CONTENT)` and `.plugin(noctis::prefix::PrefixPlugin::new("!").command::<Profile>().command::<Badge>())` with a comment; `AGENTS.md` -- one line on prefix commands (opt-in feature, intent, same handler, errors).
- Skill -- `app-and-sharding.md` plugin bullet rewritten (drop `[planned]`); new "Prefix commands (`prefix` feature, P2)" section in `commands.md` (opt-in, grammar, argument table per option type, errors table mirroring the matrix, ignored messages, refused metadata, ResolvedRole/ResolvedChannel limitation, DMs need `DIRECT_MESSAGES`); `responder.md` §4.1 points to it; `troubleshooting.md` rows for `MissingIntent MESSAGE_CONTENT` and the P1-P6 violations; `SKILL.md` index; mention every new snapshot item (`PrefixPlugin`, `new`, `command`, `CommandInput::with_error`/`error`/`with_resolved`, `OptionError::Constraint`).
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; put the printed hash in `SKILL.md` `API hash`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- `(decision)` lines for implementation-time decisions beyond D1-D8.

**Acceptance Criteria:**
- Given the `prefix` feature off, when `cargo tree -p noctis -e normal` runs, then `noctis-prefix` is absent, and an App with only slash Commands has no `MESSAGE_CONTENT` in `app.intents()`.
- Given `.plugin(noctis::prefix::PrefixPlugin::new("!").command::<Warn>())` and `.intents(Intents::MESSAGE_CONTENT)` on a `TestApp`, when the gateway fake dispatches `MESSAGE_CREATE` `!warn <@5> 3`, then the same `#[handler]` runs with the decoded options and its answer is POSTed to `/channels/{channel}/messages` referencing the invoking message.
- Given the same App, when `!warn <@5> abc` arrives, then the handler does not run, a public `noctis.validation.failed` reply is sent, and the Command Error Sink receives `OptionError::WrongType { option: "days", expected: Integer, found: String }` — the type a slash decode produces.
- Given the plugin without `MESSAGE_CONTENT` requested, when the App builds, then `BuildError` contains `[GW-05]` `MissingIntent` naming `MESSAGE_CREATE` and `MESSAGE_CONTENT`.
- Given `cargo hack test --release --workspace --each-feature` and `--all-features`, when run, then `prefix` alone and with every feature builds and tests (gate run by the loop for the epic's last story).
- Given the finished story, when `api-snapshot`, `skill-check`, `deps-check` and `rules-catalog` run, then all pass and the Usage Skill documents prefix commands as an opt-in P2 feature.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 26 findings — high 0, medium 8, low 7, false 11, maybe-false 0
- findings:
  - `[false]` `[reject]` (blind) `resolve_user` drops `Mention.member`/`message.member`, so `ResolvedUser.member` is always `None` — the intent's Never list settles it ("`members` in the resolved data stay empty"); `InteractionMember` needs `permissions`, which a message lacks; the skill documents `ResolvedUser.member` is `None`.
  - `[false]` `[reject]` (blind) DM-allowed Commands need `DIRECT_MESSAGES`, not added by the plugin — spec D6 deliberately leaves it to the author and `commands.md` §9 says "DMs also need `DIRECT_MESSAGES`, which you request yourself".
  - `[false]` `[reject]` (blind) opted-in but unmounted Command is not a build violation — intent row R40 and D1 settle it: `UnknownCommand` is ignored with a `debug` log (also needed for guild-scoped mounts in other guilds).
  - `[medium]` `[patch]` (blind) `response = ephemeral` Commands pass build checks but fail every prefix run with `EphemeralUnsupported` — verified: `Opted` does not read `T::EPHEMERALITY`, `violations` has no check, the message Responder returns `EphemeralUnsupported` (`responder.rs`), so the user gets no answer; fix: refuse at build like the other unenforceable metadata.
  - `[low]` `[reject]` (blind) `integration_types` ignored, a UserInstall-only Command runs from a guild message — the bot receiving `MESSAGE_CREATE` is guild-installed there and the Command must be opted in and mounted by the author; unlikely in everyday use and the fix adds a new check branch.
  - `[medium]` `[defer]` (blind) the template's `prefix` feature is compiled by no gate — real; grouped with the vgap template finding; deferred to Story 14.2 (template already fails to build for pre-existing reasons).
  - `[low]` `[patch]` (blind) `!pick red extra` reports `UnknownChoice { value: "red extra" }` because a rest-of-line STRING with choices compares the whole remainder — verified in `Binder::bind`/`string`; behaviour follows spec D3, so the fix documents it in `commands.md` §9.
  - `[false]` `[reject]` (blind) path words lowercase by Unicode, choice names by ASCII — both rules are fixed by the intent (rule "`str::to_lowercase`" for paths, R25 "name ASCII-case-insensitive" for choices); a localized-name clash is an author-declared ambiguity the slash client shares.
  - `[false]` `[reject]` (blind) OPT-03 reported by two variants (`OutOfRange` beyond i64, `Constraint` within i64) — intent rows R16 and R17 select exactly these two variants.
  - `[low]` `[patch]` (blind) `commands.md` §9 says "Anything left over is an error" while extra message attachments are silently ignored — verified: `Binder::bind` consumes attachments only per ATTACHMENT option; fix: say extra attachments are ignored.
  - `[low]` `[reject]` (blind) parse tests miss extra attachments, several ATTACHMENT options, bare author id for MENTIONABLE, `<#id>` for MENTIONABLE, missing-after-present — the code paths behave correctly on reading (`Binder::value`, `bind`); test-only additions with no observed defect.
  - `[medium]` `[patch]` (blind) e2e covers only one guild success, one `WrongType`, a bot message, and asserts absence after a fixed 100 ms sleep — grouped with the vgap wiring finding (same root cause: `on_message` wiring not exercised end to end); fix below.
  - `[low]` `[reject]` (blind) `is_ignored` does not check `author.system` — Discord system-user messages reaching a bot with the prefix text are not met in everyday use; the docs' "system messages" means non-Regular kinds, which are ignored.
  - `[medium]` `[patch]` (edge) ephemeral Command opted in builds, then fails every run — same defect as the blind ephemeral finding; same fix.
  - `[low]` `[reject]` (edge) `""` binds an empty STRING to a required option — real but needs a user to type an explicit empty quote; the fix adds a branch with an open design question (absent vs. error vs. skipping, which spec D3 forbids).
  - `[low]` `[reject]` (edge) `image`/`video`/`audio` `file_types` reject an attachment without `content_type` — Discord sets `content_type` for media uploads; unlikely in practice and the fix adds an extension-family table.
  - `[medium]` `[patch]` (vgap, pre-verified) `on_message`'s `contexts` argument and `guild_id` forwarding are verified by no test (inverting `allowed(..)` or dropping `.guild_id(..)` passes every test) — fix: e2e test with a `contexts = [Guild]` guild-scoped Command, a guild message (reply + recorded `Some(guild)`) and a DM message (no request), absence asserted after a later served message instead of a sleep.
  - `[medium]` `[defer]` (vgap, pre-verified) template `prefix` path compiled by no gate — deferred as filed (Story 14.2 makes `template-build` blocking); recorded in `deferred`.
  - `[false]` `[reject]` (intent D1) opt-in by type on the plugin instead of a Command flag — the intent contract's Approach and Never list (no macro argument) select R1b deliberately.
  - `[false]` `[reject]` (intent D2) core gains seams — the intent contract lists exactly these prefix-agnostic seams; no parsing code is in core.
  - `[false]` `[reject]` (intent D3) new `OptionError::Constraint` — intent contract and D2 select it for limits Discord enforces only on slash; delivered through the same notice/sink path.
  - `[false]` `[reject]` (intent D4) `MissingIntent` whenever the plugin is installed, even with zero Commands — with zero Commands P4 already fails the build, so the observable outcome (build fails) is the same under either reading.
  - `[medium]` `[patch]` (intent D5) Commands accepted at build that cannot run the same way — the ephemeral part is the real defect (grouped with the ephemeral entry); `Resolved*`/member limits and `contexts` handling are settled by spec D5 and the intent rules.
  - `[false]` `[reject]` (intent D6) no prefix test exercises stages 3-10 — the plugin enters the same `Dispatcher::run_command` whose Story 6.5 tests cover those stages; no prefix-specific stage logic exists.
  - `[false]` `[reject]` (intent D7) feature-off AC holds only by construction — the optional dependency makes it hold, and the spec's Verification runs `cargo tree -p noctis -e normal | grep -c noctis-prefix` (0).
  - `[medium]` `[defer]` (intent D8) feature-matrix AC and template prefix feature not built — `prefix` alone is the epic-last `cargo hack` gate by maintainer decision (2026-10-23); the template part is the deferred template entry.

## Design Notes

Decisions (log each as a `(decision)` line in the architecture memlog):
- **D1 Opt-in by type on the plugin**: `PrefixPlugin::new("!").command::<T>()` with `T: noctis_core::__SlashCommand` (the derive's hidden trait gives `PATHS`, `METADATA`, `Options::OPTIONS`). No macro argument, no core flag, so core stays prefix-free; `T` must also be mounted in a Module (else `UnknownCommand`, ignored with `debug`).
- **D2 Same typed errors via the input**: argument failures become `CommandInput::with_error(OptionError)`, checked in `CommandHandler::call`, so the pipeline's notice/sink path is exactly the slash decode path. Constraints Discord enforces for slash (OPT-03/04/05/06/13) are checked by the plugin and reported as the new `OptionError::Constraint`; integer overflow reuses `OutOfRange`; grammar errors reuse `Malformed`.
- **D3 Grammar**: positional, declaration order; ATTACHMENT options bind message attachments in order; the last non-ATTACHMENT option, when STRING, takes the trimmed remainder verbatim (unquoted only when the remainder is exactly one quoted token); `"…"` quoting with `\"`/`\\` escapes; no option skipping.
- **D4 Refused metadata** (P5): permissions, nsfw and channel_types cannot be enforced from a message without a cache, so opting such a Command in is a build violation instead of a silent security gap.
- **D5 Resolved data**: users from `message.mentions` and the author (`Mention` → `User`, other `User` fields `None`/defaults; verify twilight-model 0.17.1 field lists in `~/.cargo/registry`), attachments from `message.attachments`; members, roles, channels never resolved, so `ResolvedRole`/`ResolvedChannel` fields decode as `Unresolved` over prefix (documented).
- **D6 Intent**: hidden `EventHandler::__with_intents(MESSAGE_CONTENT)` + fold split, so GW-05 is the existing `MissingIntent` path; DMs additionally need `DIRECT_MESSAGES` from the author (not derived).
- **D7 Prefix rules**: 1..=32 Unicode scalars, no whitespace, case-sensitive, no whitespace before the path.
- **D8 Template**: template-local feature `prefix` (off), because a template default must not require a privileged intent.

```rust
let mut b = App::builder();
b.intents(Intents::MESSAGE_CONTENT)
 .module(Module::new("mod").add::<Warn>())
 .plugin(noctis::prefix::PrefixPlugin::new("!").command::<Warn>());
```

## Verification

**Commands** (repository root, release only):
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release -p noctis-prefix` and `cargo test --release -p noctis-core` -- pass (matrix tests)
- `cargo test --release -p noctis --features prefix,testkit --test prefix_e2e` -- pass
- `cargo test --release --workspace` -- pass
- `cargo tree -p noctis -e normal | grep -c noctis-prefix` -- 0; with `--features prefix` -- ≥1
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `api-snapshot` (after `--bless`), `skill-check`, `tw-grammar --check` -- ok
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run by the gate script (last story of Epic 15), not by the implementer.

## Auto Run Result

- **Summary:** Implemented `PrefixPlugin` in `noctis-prefix` (facade feature `prefix`, off by default). Authors opt slash Commands in by type (`PrefixPlugin::new("!").command::<T>()`). `install` validates the config (P1-P6, plus ephemeral Commands) and mounts a `MESSAGE_CREATE` handler that requires `MESSAGE_CONTENT` (GW-05). The handler parses the prefix, the Command Path and positional arguments against `OptionDecl`s and runs `Dispatcher::run_command`. Core gains only prefix-agnostic seams: `CommandInput::with_error`/`error`/`with_resolved`, `OptionError::Constraint`, the hidden `EventHandler::__with_intents`, a privileged/non-privileged intent fold split, and a hidden `__rules` re-export.
- **Files changed:** `crates/noctis-core/src/{options,command,handler,registry,pipeline,lib}.rs` and tests (seams, handler short-circuit, GW-05 mixed-intent fold); `crates/noctis-prefix/{Cargo.toml,src/lib.rs,src/parse.rs,src/parse/tests.rs}` (the plugin, tokenizer/binder, matrix tests); `crates/noctis/{Cargo.toml,src/lib.rs,tests/prefix_e2e.rs}` (optional dependency, `noctis::prefix`, e2e); `docs/discord-rules.md` (GW-05 row); `examples/project-template/` (Cargo feature, lib.rs block, AGENTS.md, Usage Skill commands/app-and-sharding/responder/troubleshooting, API hash); `xtask/{public-api.txt,src/api_snapshot.rs}`; `.github/workflows-disabled/ci.yml` (prefix feature test lines); architecture memlog.
- **Review findings:** 26 (high 0, medium 8, low 7, false 11).
  - Patched (medium 4 entries, low 2): ephemeral Commands are now an install violation, because the message Responder cannot send ephemeral replies (blind, edge and intent D5 entries grouped). A new e2e covers the `contexts` and `guild_id` forwarding wiring (vgap entry and blind e2e-coverage entry grouped). Docs: a rest-of-line STRING with choices compares the whole remainder, and extra attachments are ignored.
  - Deferred (medium 3): the template `prefix` feature path is compiled by no gate and goes to Story 14.2 (`template-build` blocking, also with `--features prefix`). The feature-matrix AC is covered by the epic-last `cargo hack` gate.
  - Rejected: member data in the resolved users (the intent says members stay empty); `DIRECT_MESSAGES` not added (D6); unmounted Command not a violation (R40); `integration_types` ignored; Unicode vs ASCII case rules (fixed by the intent); two OPT-03 variants (R16/R17); extra parse tests with no defect; `author.system`; an explicit `""` empty STRING; media `file_types` without `content_type`; intent D1-D4, D6 and D7 (each settled by the intent contract).
- **Follow-up review recommended:** true. The first pass patched 4 medium entries and no high. Named risk: a second review has not checked the new ephemeral-Command install violation or the new contexts/guild_id e2e wiring test.
- **Verification:** all local gates passed in `tools/story-loop/gates.sh` (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI is paused by the maintainer.
- **Residual risks:** no gate builds the template's prefix block until Story 14.2. `""` binds an empty STRING. `image`/`video`/`audio` file_type filters depend on `content_type`.
