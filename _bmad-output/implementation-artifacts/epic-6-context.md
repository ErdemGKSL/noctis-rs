# Epic 6 Context: Slash commands and Publish

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author declare tescord-style Commands (Command Paths and Patterns), typed options, autocomplete and context menus as plain `async fn`s, with every Discord command rule checked at compile time or at `App::build`. Handlers reply through a typestate Responder with automatic deferral, errors and panics become localized ephemeral notices, and Publish writes commands to Discord only when the Registry differs from the remote state. This is the first epic where a bot does useful slash-command work end to end.

## Stories

- Story 6.1: `#[command]` with compile-time Command Path and Pattern validation
- Story 6.2: Typed options with `derive(Options)` and `derive(Choices)`
- Story 6.3: Command metadata and context-menu Commands
- Story 6.4: App-build merge and conflict checks for Commands
- Story 6.5: Command pipeline and the Responder typestate
- Story 6.6: Automatic deferral, token lifetime and follow-up cap
- Story 6.7: Autocomplete
- Story 6.8: User-facing errors, built-in notices and panics as responses
- Story 6.9: Publish with diff, dry run and CLI modes

## Requirements & Constraints

- Command word count (1-3) decides command, subcommand, or group plus subcommand; patterns like `config (set|get) (prefix|welcome)` expand to a typed path enum the handler receives.
- Every rule failure names its rule id (CMD-*, OPT-*, META-*, CTX-*, REG-*, INT-*); compile-time checks must fail under `cargo check` and have trybuild `compile_fail` fixtures plus boundary passes (32-char name, 100-char description, 25 children/choices/options).
- Descriptions come only from literal `description = "…"`; doc comments are rustdoc only. No locale-file source in this epic (arrives in Story 11.4).
- Missing/wrong option values are typed errors, never panics; option validators reply with an ephemeral message from a `MessageKey` and skip the handler.
- Autocomplete: at most 25 choices sent (excess truncated with a warning in all modes); errors or missed deadline respond with an empty list and go to the Autocomplete Error Sink.
- Handler errors and panics: short ephemeral localized message to the user (given key or `noctis.error.generic`), full error to tracing and the Error Sink; the shard keeps running. `noctis-i18n` provides default English for reserved `noctis.*` keys.
- Publish must never burn the command-create quota: no write when remote equals local; dry run prints JSON, diff and create count and writes nothing; never publishes on shard ready.
- Metrics/spans: `noctis_autodefer_total`, `noctis_ack_latency_seconds`, `noctis.publish` span.
- Each public API change updates the Usage Skill (commands, options, metadata, Responder, autocomplete, errors, publish workflow), the project template, `api-snapshot` and `skill-check`.

## Technical Decisions

- Enforcement ladder: literal/fixed-shape checks at compile time (proc macros measuring with `noctis-rules`, traits with `#[diagnostic::on_unimplemented("[RULE-ID] …")]`, or by construction); cross-Module checks at `App::build` returning every violation at once in one `BuildError`; runtime only for runtime data.
- Pattern expansion happens in the macro via `noctis_rules::command::expand` (left to right, default cap 25, `max_expansions = n`), then `validate_tree`; the same `validate_tree` runs at App-build for merged Modules.
- Registry is immutable data folded once at `App::build` from Module descriptors; dispatch uses O(1) maps keyed by default (unlocalized) names; Publish JSON is generated only from the Registry. Modules sharing a base word merge; conflicting base/group metadata across Modules is an error; undeclared metadata takes the lexicographically first Module id's value with a warning.
- Responder: `Responder<K, S = Pending>`, aliases `CommandResponder<M = NoModal>`. `Pending` offers `reply`/`defer` (and `modal` only for `OpensModal`), consuming self into `Acked`, which offers only `edit_original`, `followup(msg, Visibility)` and `delete_original`; `done()` yields `Responded`. Commands have no `update`. Every V2 send/edit carries `IS_COMPONENTS_V2` and passes the runtime validator.
- Auto-defer: armed at `received_at + 2000 ms` (configurable), type 5 with only `EPHEMERAL`, over the direct hedged callback path; a shared atomic state machine turns a late `reply` into `edit_original`. `opens_modal` disables it and must match the `OpensModal` responder exactly (compile error otherwise). After 15 min: public edits fall back to bot-token edits, ephemeral returns `TokenExpired` without HTTP. User-installed apps cap follow-ups at 5.
- Fixed pipeline stages: resolve, Ref, enablement checks, locale, rate limit, auto-defer arm, before-Middleware, Guards, handler, after-Middleware + Error Sink. Notices after an auto-defer go as ephemeral follow-up and the deferred original is deleted. Core exposes public `noctis::Dispatcher` (via `EventCtx::dispatcher()`, `run_command`) for reuse by prefix commands.
- `#[command]` generates a `<Fn>Path` enum (with `ALL`, `as_str`, `CommandPath` trait: `PATHS`, `from_path`) only when the pattern has a group; variants are the chosen alternatives in UpperCamelCase (`Bare` for an all-absent expansion), and a name collision is a compile error. `noctis_rules::command::validate_pattern_spacing` (CMD-02 empty word, CMD-01 other whitespace) runs before `expand` in both the macro and App-build; `PathKind::of` maps word counts to path kinds.
- Core holds `CommandHandler` descriptors in `Module` (folded into the Registry only from 6.4) and a minimal `CommandCtx` (`command_path()`) that 6.5 extends. `AckDeadlineMissed { elapsed }` from the hedged callback path is the standalone error 6.5 wraps; core's `#[doc(hidden)] __testing` module is a stopgap until 6.5 exposes the path.
- Options (6.2): `CommandOption` (`KIND`/`CHOICES`/`decode`), `Options` (`OPTIONS`/`from_input`/`validate`), `CommandInput`, `OptionValue`, `OptionError` and `InvalidOption` live in `noctis-core`; `Option<T>` marks optional fields. `derive(Options)` reports kind-independent violations at expansion and kind-dependent ones as free `const _` items (E0080 under `cargo check`); `derive(Choices)` is STRING-only. Validators are sync `fn(&T, &CommandCtx) -> Result<(), MessageKey>` on present values; failure returns `InvalidOption { option, key }` and 6.8 sends the notice. Unknown incoming options are ignored; integers must fit i64. `autocomplete = path` is a flag until 6.7. CMD-* option fixtures live under `tests/ui/cmd/` (fixture dir must match the rule id's area).
- Metadata (6.3): `#[command]` accepts `permissions = A | B` (twilight flag names checked against `noctis_rules::command::PERMISSIONS`; META-01 for unknown/bad syntax), `contexts = [..]`, `integration_types = [..]`, `nsfw = true|false` (literal, else CMD-07), `guilds = [<u64 literal>, ..]` (non-zero); lists non-empty and duplicate-free; per-path forms cite META-02; `guilds` with contexts/integration_types cites META-03; `dm_permission` cites META-05. Runtime form is `#[non_exhaustive]` `noctis_core::CommandMetadata` (built in a `const`, `'static` slices, `to_rules(self)`), exposed via `metadata()` on `CommandHandler` and `ContextMenuHandler`.
- Context menus: `#[user_command("Name", ..)]` / `#[message_command("Name", ..)]` on `async fn(ctx: CommandCtx, target: ResolvedUser | ResolvedMessage) -> HandlerResult<()>` produce `ContextMenuHandler` descriptors kept in `Module`'s `menus` list for 6.4; `description` there is CTX-02; `ctx.command_path()` returns the menu name; decode failures are `MenuTargetError`. `key = ".."` stays planned (Epic 11).
- Errors: per-crate `thiserror` enums; handlers return `Result<_, HandlerError>` with optional `MessageKey`; all handler futures run under `catch_unwind`; no `unwrap`/`expect` on external input.
- Publish CLI uses a built-in parser (no clap): `--publish`, `--publish-guild <id>`, `--publish-clear`, `--dry-run`; available via `App::run`, `App::run_with_args`, and `noctis::cli::NoctisArgs::split`; unknown flags reaching `App::run` are typed errors. Publish checks the application's enabled `integration_types` first (META-04). Tests use `FakeDiscordHttp`.

## Cross-Story Dependencies

- 6.1 provides the command macro and descriptors used by 6.2-6.4, 6.7 and 6.9; 6.2 options feed 6.7's `Partial<Options>`.
- 6.5's Responder and pipeline underpin 6.6 (auto-defer state machine), 6.7 and 6.8 (notices).
- Depends on Epic 1 (`noctis-rules`, rules catalog), Epic 4 (`V2Message`, runtime validator) and Epic 5 (running App, interaction lane, hedged callback path from Story 5.9).
- Later: Story 11.4 adds locale-file descriptions; Stories 12.3/13.2 add CLI flags; Story 15.4 `PrefixPlugin` uses `Dispatcher`; Epic 7 reuses the Responder for components and modals.
