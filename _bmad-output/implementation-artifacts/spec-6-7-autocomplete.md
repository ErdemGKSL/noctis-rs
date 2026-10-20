---
title: 'Story 6.7: Autocomplete'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `#[option(autocomplete = path)]` only sets the Publish flag (Story 6.2), and the worker drops every `APPLICATION_COMMAND_AUTOCOMPLETE` (type 4) interaction (`worker.rs` `interaction_unhandled`). A bot cannot suggest values, and Discord shows "loading options failed". Implements FR-10 (autocomplete), AD-11; INT-06, INT-07, OPT-12.

**Approach:**
- `#[autocomplete] async fn f(ctx: AutocompleteCtx[, partial: Partial<O>]) -> HandlerResult<Responded>` turns into a descriptor `fn f() -> AutocompleteHandler<O>`.
- `#[derive(Options)]` links each `autocomplete = path` field to that descriptor. It also generates `<Name>Partial`, the same fields as `Option<T>`, named by `Partial<O>`. No Module registration is needed.
- A new pipeline stage, `run_autocomplete`, handles type-4 interactions:
  1. resolve the Command (3-word paths included) and the focused option's handler;
  2. decode the partial options leniently;
  3. run the handler under `catch_unwind`, a 2.5 s deadline from `received_at` and the shutdown token;
  4. truncate to 25 and drop invalid choices, with warnings;
  5. send one type-8 callback. On any failure the callback carries an empty list, and the error goes to the Autocomplete Error Sink.

## Boundaries & Constraints

**Always:**
- `AutocompleteCtx` has exactly one response method: `respond(self, choices) -> Responded`. It is synchronous and infallible, and it stores the choices for the pipeline. There is no `defer`, `reply` or Responder (INT-06).
- Exactly one initial callback per autocomplete interaction, always type 8 `{"type":8,"data":{"choices":[..]}}`. It goes over `InteractionTarget::callback(body, false)` (hedged edge path, records `noctis_ack_latency_seconds`).
- Truncation to `limits.autocomplete_choices` (25) happens in every `LimitMode` with one `warn!` on target `noctis::autocomplete`, carrying `handler`, `returned` and `sent` (INT-07).
- Kept choices go through `noctis_rules::command::validate_autocomplete(&choices, kind, &app.limits)`. Each kept choice with any violation (`choices[j]` path, OPT-09/OPT-03/OPT-04/OPT-05) is dropped. One `warn!` (`noctis::autocomplete`, `handler`, `dropped` count, the rule ids) covers the whole response. An OPT-11 kind means an empty list. Never an error for the user.
- Every `Partial` field is `Option<T>`. Decoding never fails: a missing, mistyped, unresolved or unknown-choice value is `None` (OPT-12). Validators (`validate = path`) never run on a `Partial`.
- Deadlines use `tokio::time::Instant` from `Inbound::received_at` (paused clocks control them). `AUTOCOMPLETE_DEADLINE = 2500 ms`.
- Logs and metric labels carry no ids, tokens, focused text or choice text (AD-29). Metrics are `noctis_handler_duration_seconds` and `noctis_handler_errors_total` with `kind="autocomplete"`. There is no new metric.
- Library code has no `unwrap`/`expect` on external input. No `std::sync::Mutex` guard is held across `.await`.
- Test names: `int_06_*` and `opt_12_*` unit tests must exist. Tagged `compile_fail` doctests: `// [INT-06]` and `// [OPT-12]` (rules-catalog by-construction).

**Never:**
- No autocomplete over the `Dispatcher`. No async/fallible `respond`. No configurable deadline. No `AppBuilder` setter.
- No `Module::add` for autocomplete descriptors. They are not `Descriptor`s.
- No component or modal dispatch (Epic 7), no notices or `noctis.*` keys (6.8), no Publish (6.9), no testkit `app.autocomplete(..)` builder (Epic 8), no localized choice names (Epic 11).
- No new crate dependency or dependency edge. No debug builds. No real sleeps over 1 s in tests.

## I/O & Edge-Case Matrix

Abbreviations:
- **CB(x)**: one type-8 callback with `choices` = x.
- **S**: Autocomplete Error Sink report (origin `Autocomplete`, module and handler set).
- **D**: `received_at + 2500 ms`.

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Happy, 3-word path | `config set prefix`, focused `value`="pr", handler returns 3 STRING choices | CB(3 choices in order, `{"name","value"}`) | none |
| 2 | Partial typing | other options `channel`=valid id with resolved data, `limit` absent (required) | `partial.channel == Some(..)`, `partial.limit == None` | none |
| 3 | Partial lenient | non-focused INTEGER sent as `"abc"`, unknown Choices value, id missing from resolved, option not in struct | those fields `None`; unknown option ignored; handler runs | none |
| 4 | Focused String | focused STRING `""` / `"çğ😀"` | `ctx.focused().value()` exact text; `partial.<field> == Some(text)` | none |
| 5 | Focused INTEGER | raw `"12"` / `"-"` / `"99999999999999999999"` / JSON number `12` | `as_integer()` = `Some(12)` / `None` / `None` / `Some(12)`; partial field mirrors it | none |
| 6 | Focused NUMBER | `"1.5"` / `"NaN"` / `"inf"` | `as_number()` = `Some(1.5)` / `None` / `None` | none |
| 7 | 0 choices | `respond([])` / `respond(Vec::new())` | CB(`[]`) | none |
| 8 | 25 choices | exactly 25 | CB(25), no warn | none |
| 9 | 26 / 30 choices (INT-07) | Strict and Lenient | CB(first 25); one warn with `returned=26`/`30`, `sent=25` | none |
| 10 | Name length | name 1, 100, 101 chars; 0 chars; 50 vs 51 × `𝔸` (UTF-16 pairs) | 1/100/50 kept; 101, empty, 51 dropped; one warn `dropped=n` citing OPT-09 | none |
| 11 | String value length | value 0, 100, 101 chars | 0 and 100 kept, 101 dropped | none |
| 12 | Value type | INTEGER option with `AutocompleteValue::String`; NUMBER option with `Integer(3)` | String dropped (OPT-09); `Integer(3)` kept (fits NUMBER) | none |
| 13 | Numeric range | INTEGER `2^53` / `2^53-1`; NUMBER `f64::NAN`, `f64::INFINITY` | `2^53-1` kept; the others dropped | none |
| 14 | Truncate before check | 30 choices, #27 invalid, #3 invalid | CB(24): first 25 minus #3; both warns | none |
| 15 | Handler `Err` | `Err(HandlerError)` | CB(`[]`) | S `Error`; `errors_total{kind=autocomplete}` +1 |
| 16 | Handler panics | panic in body or at call | CB(`[]`) | S `Panic` |
| 17 | Deadline missed | handler sleeps past D (paused clock) | handler future dropped; CB(`[]`) right after D | S `Error(AutocompleteError::DeadlineMissed)` |
| 18 | Already late | `received_at` 2.6 s ago at stage start | handler not run; CB(`[]`) attempted | S `DeadlineMissed`; an edge `DeadlineMissed` is only a warn |
| 19 | Shutdown | cancel token fires mid-handler | CB(`[]`) | S `Cancelled` |
| 20 | Responded without respond | handler returns a `Responded` whose ctx never called `respond` (smuggled) | CB(`[]`) | none |
| 21 | Callback fails after success | FakeDiscordHttp 400 | no retry | S `Error(ResponderError::Rejected)` |
| 22 | Callback fails after failure | handler `Err`, callback 400 | — | S only for the handler error; callback failure `warn!` `noctis::autocomplete` |
| 23 | Unknown command path | not in Registry for the scope | CB(`[]`); warn `noctis::dispatch` "unknown command" | no S |
| 24 | Focused option without handler | option not linked / option of a menu | CB(`[]`); warn "no autocomplete handler" | no S |
| 25 | No / two focused | 0 or 2 options with `focused:true` | CB(`[]`) | S `Error(OptionError::Malformed)` |
| 26 | Malformed JSON shape | `data.options` not an array | CB(`[]`) | S `Malformed` |
| 27 | OPT-11 kind | focused option type BOOLEAN in payload | CB(`[]`), warn | none |
| 28 | Plain handler | `#[autocomplete] async fn f(ctx: AutocompleteCtx)` on a field | compiles for any `O`; runs | none |
| 29 | Not a handler path | `autocomplete = plain_fn` (not `#[autocomplete]`) | E0277 "`…` is not an `#[autocomplete]` handler for `O`" | compile error |
| 30 | Wrong Options type | handler takes `Partial<Other>` | E0277 same message naming `O` | compile error |
| 31 | Bad signatures | non-async; generic; `self`; 0 or 3 params; first param not `AutocompleteCtx`; any macro argument | `syn::Error`s: "`#[autocomplete]` needs an `async fn`", "… cannot be generic", "`#[autocomplete]` handlers take `(ctx: AutocompleteCtx[, partial: Partial<Options>])`", "`#[autocomplete]` takes no arguments" | compile error |
| 32 | Wrong return | `-> HandlerResult<()>` | E0271/E0277 at the return type | compile error |
| 33 | INT-06 | `ctx.defer()` on `AutocompleteCtx` | E0599 | compile_fail doctest `// [INT-06]` |
| 34 | OPT-12 | `let n: i64 = partial.limit;` | E0308 | compile_fail doctest `// [OPT-12]` |
| 35 | Ack latency | CB at +120 ms | `noctis_ack_latency_seconds` sample ≈ 0.12 | none |

</intent-contract>

## Code Map

**Core (`crates/noctis-core/src/`)**
- `options.rs`:
  - `Options` trait (L72): add `type Partial: PartialOptions<Options = Self>;` and `fn autocomplete(option: &str) -> Option<AutocompleteHandler<Self>> { None }`.
  - `CommandInput::from_json` (L294) plus the private `walk_path`/`leaf_options`/`decode_value` (L451-555): factor them to also parse type 4.
  - `__options::{required, optional, choice}` (L640+): add `partial`.
- `command.rs`:
  - `CommandHandler` (L313) and its four constructors (L350-470). `decode::<O>`.
  - `CtxInner` (L83) and the `CommandCtx` accessors (L156-227) are the pattern for `AutocompleteCtx`, including `state` and `http`.
- `pipeline.rs`:
  - `resolve` (L196) already walks `SubCommandGroup`/`SubCommand`, and twilight parses type-4 data as `InteractionData::ApplicationCommand`.
  - `Report`, `Outcome`, `execute` (L245) are the run/sink/metrics pattern. `run_interaction` (L368) is the parts pattern.
- `worker.rs` L126-145: `Workers::interaction` routes only `ApplicationCommand`. Add `ApplicationCommandAutocomplete` → `run_autocomplete` on the tracker.
- `responder.rs` L353: `Responded { _private: () }` (crate-constructible).
- `responder/transport.rs`: `InteractionTarget` and `callback(body, with_response)` (L240).
- `edge/callback.rs` L31: `ACK_DEADLINE` 2900 ms (the edge refuses later callbacks).
- `sink.rs`: `ErrorOrigin::Autocomplete` (L33, target `noctis::autocomplete`) and `ReportKind`.
- `handler.rs`: `HandlerError::new`, `AppShared { limits, .. }`.
- `lib.rs` L184: the re-exports.

**Rules:** `noctis_rules::command::{validate_autocomplete, Choice, ChoiceValue, OptionType::is_choosable}` (`command/option.rs` L891). The tests `int_07_*` already exist there.

**Macros (`crates/noctis-macros/src/`)**
- `options.rs`:
  - `const_checks` L553-570: the `let _ = #path;` existence check gets replaced.
  - `expand` L645-727: add the Partial struct and the `autocomplete` fn.
  - Unit tests L1218-1260.
- `menu.rs` `expand` L209-273: the descriptor-fn pattern (inner fn renamed, constructor spanned at the return type).
- `command.rs` `check_shape` L219: reuse it.
- `lib.rs` L170-210: the attribute entry points.

**Facade, tests, repo**
- `crates/noctis/src/lib.rs`:
  - re-exports L390, `Options` docs and the `#[option]` table (L465);
  - `pub use noctis_macros::command` (L687) pattern;
  - prelude L1101-1147.
- `crates/noctis/tests/options.rs`: L98 `fn suggest() {}` and L250 test.
- `crates/noctis/tests/ui/opt/OPT-10_choices_and_autocomplete.rs` and `OPT-11_autocomplete_on_bool.rs`: they use a dummy path.
- `crates/noctis-testkit/tests/commands.rs`: `Harness`, `invoke` L197, `wait_sent` L223, `wait_report` L235.
- `crates/noctis-core/src/pipeline/tests.rs`: `Harness::start_with` L58, `requests` L98, `inbound_raw` L149.
- `xtask/rules-pending.toml`: L16-24 (OPT-12 by-construction/unit) and L61-69 (INT-06 by-construction/unit).
- `docs/discord-rules.md`: L113 (OPT-12), L161 (INT-06), L162 (INT-07).
- Template:
  - `.claude/skills/noctis/references/commands.md` §4 (L525-576) and the L364 table row;
  - `responder.md` L119;
  - `modules-middleware-ratelimits.md` L40;
  - `troubleshooting.md` L117;
  - `limits-cheatsheet.md` L186/L210;
  - `SKILL.md` (L311, API hash L21);
  - `AGENTS.md`;
  - `src/modules/profile.rs`.

## Tasks & Acceptance

**Execution (in order):**

1. **`crates/noctis-core/src/autocomplete.rs` (new; `mod autocomplete;` in `lib.rs`).** Public, documented, with doctests:
   - `Focused` (`Debug, Clone`): `name() -> &str`, `kind() -> OptionType`, `value() -> &str` (raw text; a JSON number is kept as its text), `as_integer() -> Option<i64>` (`str::parse`), `as_number() -> Option<f64>` (parse, finite only).
   - `#[non_exhaustive] enum AutocompleteValue { String(String), Integer(i64), Number(f64) }`, with `From<&str>`, `From<String>`, `From<i64>` and `From<f64>`.
   - `AutocompleteChoice` (`Debug, Clone, PartialEq`): `new(name: impl Into<String>, value: impl Into<AutocompleteValue>)`, `name()`, `value()`.
   - `AutocompleteCtx` (not `Clone`; `Debug` without the token):
     - accessors `command_path() -> &'static str`, `module`, `shard`, `guild_id`, `channel_id`, `user_id`, `user_locale`, `guild_locale`, `focused() -> &Focused`, `state::<T>()`, `http()`;
     - `pub fn respond(self, choices: impl IntoIterator<Item = AutocompleteChoice>) -> Responded`. It keeps the first `limits.autocomplete_choices` and counts the rest. It stores them in a shared slot (`Arc<std::sync::Mutex<Option<Collected>>>`, never held across `.await`).
     - Docs: a compile_fail doctest containing `ctx.defer(); // [INT-06]`.
   - `#[non_exhaustive] enum AutocompleteError { DeadlineMissed { deadline: Duration } }` (`thiserror`, "the autocomplete handler did not respond within {deadline:?}").
   - `PartialOptions: Default + Send + 'static`: `type Options: Options;` and `#[doc(hidden)] fn __from_input(input: &CommandInput) -> Self`. `pub type Partial<O> = <O as Options>::Partial;`.
   - `AutocompleteHandler<O>` (`Clone`, `Debug` = name), `name() -> &'static str`.
     - Hidden constructors: `__new::<P: PartialOptions, F, Fut>(name, f) -> AutocompleteHandler<P::Options>`, where `F: Fn(AutocompleteCtx, P) -> Fut`; and `__new_plain::<O: Options, F, Fut>(name, f) -> AutocompleteHandler<O>`, where `F: Fn(AutocompleteCtx) -> Fut`. Both require `Fut: Future<Output = HandlerResult<Responded>> + Send + 'static`.
     - The erased call decodes `P` from the input inside the call.
   - Hidden `trait __AutocompleteFor<O>`, blanket-implemented for `F: Fn() -> AutocompleteHandler<O>`, with `#[diagnostic::on_unimplemented(message = "`{Self}` is not an `#[autocomplete]` handler for `{O}`", label = "…", note = "annotate an `async fn(ctx: AutocompleteCtx[, partial: Partial<{O}>]) -> HandlerResult<Responded>` with `#[autocomplete]`")]`.
   - Hidden `fn __autocomplete_for<O: Options, F: __AutocompleteFor<O>>(f: F) -> AutocompleteHandler<O>`.
   - `pub(crate)`:
     - `AutocompleteInput::from_json(raw) -> Result<(CommandInput, Focused), OptionError>`: interaction type 4 and `data.type` 1, else `NotSlashCommand`. Exactly one `focused:true` leaf, else `Malformed`. Non-focused leaf values that fail `decode_value` are skipped. The focused value is inserted into the input as a typed `OptionValue` only when it parses for its type.
     - `filter_choices(collected, kind, &Limits) -> (Vec<AutocompleteChoice>, Warnings)` (Matrix 7-14, 27).
     - The type-8 body builder.

2. **`crates/noctis-core/src/options.rs`, `command.rs`**
   - Add the two `Options` items (Code Map) and `__options::partial<T>(input, name) -> Option<T>` (any error → `None`).
   - `CommandHandler` gains `autocomplete: fn(&str) -> Option<ErasedAutocomplete>`. The `_with_options` constructors set it from `O::autocomplete`; the others set `|_| None`.
   - Add `pub(crate) fn autocomplete_for(&self, option: &str)`.
   - `Debug` unchanged.

3. **`crates/noctis-core/src/pipeline.rs`, `worker.rs`**
   - `pub(crate) async fn run_autocomplete(registry, app, inbound, cancel)` runs in span `noctis.interaction` (same fields as commands). Steps:
     1. `resolve`; unknown → CB(`[]`) + warn (Matrix 23);
     2. parse `AutocompleteInput` (failure → CB(`[]`) + S);
     3. find the handler by focused name (none → Matrix 24);
     4. if `now >= D`, skip the run (Matrix 18);
     5. otherwise build the ctx and run under `catch_unwind` + `tokio::time::timeout_at(D)` + `cancel` (biased select);
     6. record the duration;
     7. filter, send the callback, log the warnings;
     8. report per Matrix 15-22 (send first, then report).
   - Worker: route `InteractionType::ApplicationCommandAutocomplete` to it on the tracker (no permit). Update the docs of `interaction` and `interaction_unhandled`, and the pipeline module docs with an "Autocomplete" paragraph.

4. **`crates/noctis-macros/src/autocomplete.rs` (new) + `lib.rs`**
   - `#[proc_macro_attribute] pub fn autocomplete`. Args must be empty. Use `check_shape(.., "#[autocomplete]")`.
   - Params: the first is typed `AutocompleteCtx` (last path segment), plus an optional second `P`. Any other shape gets the Matrix 31 message.
   - Expansion follows `menu.rs`:
     - with `P`: `#vis fn name() -> ::noctis::__private::core::AutocompleteHandler<<P as ::noctis::__private::core::PartialOptions>::Options>` calling `__new::<P, _, _>`;
     - without: `#vis fn name<__O: ::noctis::__private::core::Options>() -> AutocompleteHandler<__O>` calling `__new_plain`.
     - The name is `concat!(module_path!(), "::", fn_name)`. Keep the attrs. Add `#[allow(clippy::unused_async)]` on the inner fn. Span the constructor at the return type (Matrix 32).
   - Unit tests for every Matrix 31 message and both expansions.

5. **`crates/noctis-macros/src/options.rs`**
   - Replace `let _ = #path;` with nothing; the check moves into the generated `autocomplete` fn, spanned at `path`. Fields: `"name" => Some(#core::__autocomplete_for::<Self, _>(#path))`, others `None`.
   - Generate `#vis struct #{Name}Partial` with `#[derive(Default)]` and `#[allow(dead_code)]` (an unused Partial must not warn in bot crates or under `-D warnings`):
     - a doc "Every option of [`Name`] as an `Option` (OPT-12); what autocomplete handlers get";
     - each field keeps its vis and `#[doc]` attrs, with type `Option<T>` (T = inner type);
     - `impl PartialOptions` with `__from_input` using `__options::partial`.
     - Set `type Partial = #{Name}Partial;`.
   - Update the unit tests (L1244-1260).

6. **`crates/noctis/src/lib.rs`, `crates/noctis-core/src/lib.rs`**
   - Re-export `AutocompleteCtx`, `AutocompleteChoice`, `AutocompleteValue`, `AutocompleteHandler`, `AutocompleteError`, `Focused`, `Partial`, `PartialOptions` and the hidden items. Add `pub use noctis_macros::autocomplete` with docs and an example.
   - Add a compile_fail doctest with `let limit: i64 = partial.limit; // [OPT-12]` and a compile_fail doctest for Matrix 29.
   - Prelude: add `autocomplete`, `AutocompleteCtx`, `AutocompleteChoice`, `Partial`.
   - Update the `#[option]` table row for `autocomplete`.

7. **Tests**
   - `crates/noctis-core/src/autocomplete.rs` unit tests: Matrix 4-14, 25-27. Include `opt_12_partial_input_is_lenient` (Matrix 3, 5) and `int_07_truncates_in_both_modes`.
   - `crates/noctis-core/src/pipeline/tests.rs`: Matrix 15-24, 35 with a hand-built handler. Include `int_06_deadline_missed_sends_empty_choices` (Matrix 17, `tokio::time::pause`), `int_06_already_late_never_runs` (18) and a `Capture` of the warnings.
   - `crates/noctis/tests/autocomplete.rs` (new): the derive + macro surface (Matrix 2, 28; `Partial<O>` field types; `CommandHandler` linking via `#[command]`).
   - `crates/noctis/tests/options.rs`: `suggest` becomes `#[autocomplete] async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> { Ok(ctx.respond(Vec::new())) }`.
   - Update `OPT-10`/`OPT-11` fixtures to use a real `#[autocomplete]` fn and re-bless (`TRYBUILD=overwrite`, stderr must still contain `[OPT-10]`/`[OPT-11]`).

8. **`crates/noctis-testkit/tests/commands.rs`** -- E2E through FakeGateway + FakeDiscordHttp:
   - `#[command("config (set|get) prefix", ..)]` with options `{ value: String (autocomplete), limit: i64 }`;
   - the type-4 interaction for `config set prefix`, focused `value`;
   - assert one `POST /interactions/{id}/{token}/callback` with `{"type":8,…}` and that the handler saw `partial.limit == None` (AC 1);
   - a 30-choice handler → 25 (AC 2);
   - an erroring handler → empty choices + sink report `autocomplete` (AC 3).

9. **`xtask/rules-pending.toml`**: delete the four `story = "6.7"` entries.
   - **`docs/discord-rules.md`**: How text for OPT-12 (`Partial<O>`/`<Name>Partial`, lenient decode), INT-06 (`AutocompleteCtx::respond` only; 2.5 s deadline → empty list + Autocomplete Error Sink), INT-07 (replace the `try_push` text: `respond` keeps 25, `noctis::autocomplete` warn, invalid kept choices dropped).

10. **`examples/project-template/`**
    - `commands.md` §4: rewrite with the landed API: sync `ctx.respond(..)` (no `.await`), `ctx.focused()`, `Partial<O>`, `AutocompleteChoice`, no Module registration, 2.5 s deadline, dropped-invalid rule. Also the L364 row.
    - `responder.md` L119: drop "[planned, Story 6.7]".
    - `modules-middleware-ratelimits.md` L40: `#[autocomplete]` is not added to Modules.
    - `troubleshooting.md`: rows for "is not an `#[autocomplete]` handler", `DeadlineMissed`, and the INT-07 warn text.
    - `limits-cheatsheet.md` as needed.
    - `src/modules/profile.rs`: add `#[command("badge", description = "Show a badge", contexts = [Guild])]` with `BadgeOptions { #[option(description = "Badge name", autocomplete = badge_suggest)] name: String }` and an `#[autocomplete]` fn filtering a static list by `ctx.focused().value()`; register `badge()`.
    - `AGENTS.md`: one bullet.
    - Re-bless `xtask/public-api.txt` and copy the hash into `SKILL.md`.
    - Add `(decision)` lines to the architecture memlog for implementation choices.

**Acceptance Criteria:**
- **3-word path:** Given `#[option(autocomplete = h)]` on a field of a 3-word Command Path's options, where `h` is an `#[autocomplete]` fn, when a type-4 interaction arrives through the FakeGateway, then `h` receives an `AutocompleteCtx` whose `focused()` holds the focused name and raw value and a `Partial<O>` whose fields are all `Option<T>`, and exactly one type-8 callback with `h`'s choices is recorded.
- **INT-07:** Given a handler returning 30 choices, when it responds, then exactly 25 are sent and one warning is logged, in both Strict and Lenient mode.
- **Failure:** Given a handler that errors, panics, or misses the 2.5 s deadline, when the deadline passes, then one type-8 callback with an empty `choices` list is recorded and the Autocomplete Error Sink receives the error.
- **Compile time:** Given `ctx.defer()` on `AutocompleteCtx`, or a non-`Option` use of a `Partial` field, or an `autocomplete = path` that is not an `#[autocomplete]` handler for that Options type, when compiled, then compilation fails.
- **Gates:** Given the gates, when they run, then `rules-catalog` passes with the four 6.7 entries removed, and `api-snapshot` and `skill-check` pass with the Usage Skill documenting autocomplete with an example.

## Spec Change Log

## Review Triage Log

## Design Notes

**Sync `respond` (decision).** The pipeline is the only sender, so there is no race between a handler's callback and the deadline fallback. Exactly one type-8 callback is sent. Truncation, validation and the empty fallback live in one place. `respond(self)` consumes the ctx, so a second `respond` is impossible. The template's planned `.await?` is dropped.

**Linking through the Options type (decision).** Discord sends autocomplete per option, and the derive already knows each option's `autocomplete = path`. So `Options::autocomplete(name)` is the lookup. Module registration would only add an orphan class of errors. The `__AutocompleteFor<O>` bound turns a wrong path or a wrong `Partial<O>` into one readable E0277.

```rust
#[autocomplete]
async fn prefix_suggest(ctx: AutocompleteCtx, partial: Partial<PrefixOptions>) -> HandlerResult<Responded> {
    let typed = ctx.focused().value().to_owned();
    let limit = partial.limit.unwrap_or(5);
    Ok(ctx.respond(["!", "?", "."].into_iter().filter(|p| p.starts_with(&typed)).take(limit as usize)
        .map(|p| AutocompleteChoice::new(p, p))))
}
```

**`Partial<O>` as a generated struct (decision).** A generic `Partial<O>` cannot have per-`O` fields. `<Name>Partial` plus the alias keeps field access (`partial.limit`). A handler signature written `Partial<O>` resolves to it. Rust infers `O` from the concrete struct through `PartialOptions::Options`. The struct derives only `Default`: `Debug`/`Clone` would fail for field types without them.

**Inference of the plain form.** `__autocomplete_for::<Self, _>(path)` infers the generic descriptor's `__O` from the `Fn() -> AutocompleteHandler<O>` bound. The Matrix 28 test proves it. If inference fails, the derive may emit `path()` into a `let h: AutocompleteHandler<Self> = …;` binding instead, keeping the `__AutocompleteFor` check for the typed form.

**Deadline 2.5 s, fixed (decision).** Discord allows 3 s, and the edge refuses after 2.9 s. 2.5 s leaves time for the type-8 callback. `ack_deadline` is not reused: its range includes 0.

**Dropping invalid choices (decision).** FR-10 says autocomplete has no user-visible failure worth blocking on. Discord rejects a whole response with one bad choice, so dropping that choice is better than sending an empty list.

**Unknown command or option (decision).** Answer with an empty list (no "loading failed") and a warn, with no sink report: there is no handler to blame.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, including `int_06_*`, `opt_12_*`, `int_07_*`, `noctis --test ui` (OPT-10/OPT-11 re-blessed), `noctis --test autocomplete`, `noctis-testkit --test commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok, no 6.7 pending entries
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: ok; copy the hash into SKILL.md (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok
