# Troubleshooting

Find the symptom, apply the fix. Errors name a rule id (`[CID-01]`); look it up in
`limits-cheatsheet.md`. Never work around a check by reading or patching Noctis source;
if the skill doesn't explain an error, say so.

Build, test and run with `--release` only (`cargo build --release`, `cargo test
--release`, `cargo clippy --release --all-targets -- -D warnings`, `cargo run
--release`). If the disk fills, delete `target/debug/`.

## Compile errors

| Error | Cause | Fix |
| --- | --- | --- |
| ``[CID-01] route `x` has unbounded fields`` (`` = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to #[route] ``) | `String`/`Vec` in an inline Route | `by_ref` (or `overflow = spill`), or store the data via a port and carry an id; `BoundedStr<N>` only for tiny ASCII |
| ``[CID-01] route `x` can exceed 100 characters: worst case 101`` (`` = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown ``) | too many/wide fields, long id | shorten the Route id, use `PayloadEnum`/smaller ints, or `by_ref`; `cargo test --release -- --nocapture noctis_route_width` prints the breakdown |
| ``[CID-01] `f32` is not a Payload field type`` | a field type without `Field` | use a built-in field, implement `Field` + `FieldDecode`, or derive `PayloadEnum` |
| `[CID-01] a Route cannot be generic` (also `a Payload` / `a PayloadEnum`) | type, lifetime or const parameters, or a `where` clause | make the struct concrete |
| `[CID-01] a Route must be a struct, not an enum` (or `not a union`) | `#[derive(Route)]`/`#[derive(Payload)]` on an enum | a struct; `PayloadEnum` for unit enums |
| `[CID-01] a PayloadEnum has at most 62 variants (one base62 digit)` / ``PayloadEnum variant `B` has fields`` / `a PayloadEnum needs at least one variant` / `PayloadEnum must be derived on an enum` | variant 63, a tuple/struct variant, an empty enum, or a struct | split the enum or use an integer field; unit variants only |
| ``[CID-03] route id: limit [a-z0-9._-], actual `Shop` `` with ``= note: `S` at byte 0 is not allowed`` | uppercase, spaces, `:`, `,`, `~`, `!`, non-ASCII | `[a-z0-9._-]` |
| `[CID-03] route id: limit between 1 and 24, actual 25` | empty or too long id | 1 to 24 characters |
| ``[CID-03] route id: limit an id that does not start with `_`, actual `_x` `` | `_` prefix is reserved for Noctis | rename |
| `[CID-03] #[derive(Route)] needs a #[route("id")] attribute` / `a Route takes exactly one #[route] attribute` / `#[route] takes a list: the route id, then codec arguments` / `#[route] is missing the route id` / `the route id must be a string literal` | attribute missing, doubled or malformed | `#[route("shop.buy", args…)]` |
| ``[CID-03] unknown #[route] argument `colour` `` | typo or unsupported argument | codec arguments: `by_ref`, `overflow = spill`, `schema = n`, `select`, `signed` (`view` is withdrawn: delete it; tabs and pages are Payload fields) |
| ``[CID-03] `ttl` is a runtime argument of #[route] that this Noctis release does not support yet`` (also `one_shot`, `owner`, `permissions`, `guard`, `opens_modal`) | runtime arguments are [planned] | remove it until the release that supports it |
| ``[CID-03] #[route] argument `select` is given twice`` / ``[CID-03] `select` is a flag and takes no value`` / ``malformed #[route] argument `by_ref` `` | duplicate argument, `select = true`, `by_ref()` | write each flag once, bare |
| ``[CID-03] `overflow = error` is not a #[route] argument`` (`` = help: the plain inline Route (no `overflow`) is the error policy: leave `overflow` out ``) / ``the only `overflow` policy is `spill` `` / ``[CID-03] `overflow` needs a value`` | another overflow policy | `overflow = spill`, or nothing |
| ``[CID-03] `schema` must be in 1..=61 (one base62 digit), found `62` `` / ``[CID-03] `schema = 0` is the default schema`` (`` = help: leave it out for schema 0 ``) / ``[CID-03] `schema` must be an integer literal`` / ``[CID-03] `schema` needs a value`` | schema out of range or not an integer literal | `schema = 1` … `schema = 61`; no `schema` for 0 |
| ``[CID-03] `by_ref` and `overflow = spill` cannot be combined`` | both body policies | pick one |
| ``[CID-03] a `by_ref` route cannot be `signed` `` | a Ref is already unguessable | drop `signed` |
| conflicting implementations of `Payload` | `#[derive(Route)]` and `#[derive(Payload)]` on one struct | keep only `Route`; it includes the Payload |
| ``[CID-04] Route `X` has no handler, so no component can be built for it`` | building a component from a Route with no `#[handler]` | write the handler (and `.add(it())` to a Module) |
| ``route `X` is not declared `select` `` | `.string_select(..)` / `.user_select()` … on a Route without `select` | `#[route("id", select)]` |
| `[CMD-11] config set: limit a subcommand or a subcommand group, not both, actual \`set\`` followed by `= note: \`config set\` makes \`set\` a subcommand` and `= note: \`config set prefix\` makes \`set\` a subcommand group` | optional group creating `a b` and `a b c` | remove `?`, or move the bare form to its own path (`config show`) |
| `[CMD-12]` base invokable with children | `"x (y)?"` or `x` + `x y` | give the bare form its own word |
| `[CMD-10]` more than 3 words | path too deep | flatten; use options or a View |
| `[CMD-16]` malformed pattern, `= note: at byte N` | glued group (`a(b)`), space inside a group, nested or unclosed group, empty alternative, `?`/`\|`/`)` outside a group | fix the token at that byte offset: groups are whole words `(a\|b)` or `(a\|b)?` with single-word alternatives |
| `[CMD-15]` duplicate or too many expansions | two expansions identical (`(a\|a)`, `(a)? (a)?`) or more than `max_expansions` (25) | drop the duplicate alternative or the redundant `?`; split the pattern or raise `max_expansions` |
| `[CMD-01]` / `[CMD-03]` | uppercase, ASCII `'`, punctuation in a name | lowercase; `ʼ` (U+02BC) if you need an apostrophe |
| `[CMD-04] missing description` | no `description = "…"`, and (with the `i18n` feature) no `cmd.<path>.description` in the default locale | add it to `locales/en-US.yaml`, or the literal without `i18n` |
| description literal differs from the locale file | both sources set, with different text | delete the literal; the locale file is the source with `i18n` |
| `[OPT-01] required option after optional` | field order | move required fields first (never reordered for you) |
| `[OPT-10] choices and autocomplete` | both on one option | pick one |
| ``[OPT-05] days: limit min_value ≤ max_value, actual `6 > 5` `` / `[OPT-05] name.min_value: limit min_value / max_value only on INTEGER or NUMBER options` / `limit a choice value within min_value and max_value` | swapped bounds, bounds on a non-number, or a choice outside the bounds | swap the bounds; drop them on non-numbers; fix the choice |
| `[OPT-06] reason.max_length: limit between 1 and 6000, actual 0` / `limit min_length ≤ max_length` / `only on STRING options` | length out of range, swapped, or on a non-string | `min_length` 0–6000, `max_length` 1–6000, `min ≤ max`, strings only |
| `[OPT-08] name.choices: limit at most 25, actual 26` / `limit choices only on STRING, INTEGER or NUMBER options` | too many choices or choices on another type | ≤ 25 choices; use autocomplete for more; choices only on string/integer/number |
| `[OPT-09] name.choices[0]: limit between 1 and 100, actual 0` with `= note: in the choice name`, or `limit a choice value of the option's type, actual \`integer\`` with `= note: option type STRING` | empty/long choice name, long string value, or wrong value type | names 1–100, string values ≤ 100; `ChoiceValue::Str` for STRING, `Int` for INTEGER, `Int`/`Float` for NUMBER |
| ``[OPT-13] evidence.file_types[0]: limit `image`, `video`, `audio` or a dot-prefixed extension, actual `pdf` `` | bare extension, multi-dot extension, wrong case of a keyword | `".pdf"`, `"image"`; ≤ 10 entries, attachment options only |
| ``[META-01] config: limit one default_member_permissions per base command, actual `config` `` (also `[META-02] … one contexts value per base command`, `[CMD-07] … one nsfw value per base command`) with ``= note: `config get` leaves it unset (from `m2`)`` | leaves under one base disagree (unset counts unless a base declaration sets it) | declare it once on the base: `CommandDecl::meta("m", "config").metadata(…)` / `#[command(…)]` on every leaf with the same value |
| ``[META-02] config set: limit metadata only on the base command`` with ``= help: declare it on `config` `` | metadata on a meta declaration below the base | move it to the base path |
| ``[META-03] ping.contexts: limit only on global commands, actual `[Guild]` `` with `= note: scope: guild 42` | `contexts` / `integration_types` on a guild-scoped command | drop them for guild commands, or publish globally |
| `[CTX-01] Show …: limit between 1 and 32, actual 33` | context-menu name empty or longer than 32 | shorten it (mixed case and spaces are fine) |
| ``[CV2-05] `Container` cannot be a Container child`` / ``… cannot be a top-level message component`` / ``… cannot be an Action Row button`` | illegal nesting: Container in Container, Thumbnail outside a Section accessory, button outside a row or accessory | flatten; one Container per View; Thumbnails go in `Section::new(.., thumbnail)`, buttons in `ActionRow::buttons` |
| `[CV2-06]` unsatisfied bound in `ActionRow::buttons` | 0 or > 5 buttons, or button + select | split rows; selects alone |
| ``[CV2-07] `X` is not a Section accessory`` / Section arity | no accessory (e.g. `()`), or 0 or > 3 texts | pass a button or Thumbnail; merge texts; use a plain `TextDisplay` if there is no accessory |
| mismatched types: expected `AttachmentRef`, found `&str` on `File::new` | hand-written `attachment://` name (CV2-12) | `File::new(set.add(Attachment::new(bytes, "png")?))` |
| `[CV2-08] label too long` | literal > 80 | shorten (guideline ≤ 34 with emoji) |
| `[MOD-02] more than 5 fields` | modal too big | split into a step flow with two modal steps (`views-and-recipes.md` §3.3) |
| `[MOD-07]` radio options | enum with < 2 or > 10 variants | adjust the `Choices` enum |
| unknown Tailwind class `bg-gradient-to-r`, help: `bg-linear-to-r` | v3 name | use the v4 name (see `images.md` §4) |
| unknown class `bg-brand` | token missing from `noctis-theme.css` | add `--color-brand` under `@theme` |
| `[LOC-01] unknown locale file` | file name is not exactly a Discord locale code (`en.yaml` is not one) | rename (`tr.yaml`, `pt-BR.yaml`, `en-US.yaml`) |
| missing key / wrong number of arguments on `s.x.y(..)` | key absent in `en-US.yaml` or placeholders differ | add the key; match placeholders |
| `HashMap` field in a Template | non-deterministic cache key | `BTreeMap` or `Vec` |
| a handler taking a Responder doesn't return `Responded` | macro contract | return `acked.done()` |
| `modal` not found on `ComponentResponder` | Route not `opens_modal`, or the handler takes `ComponentResponder` instead of `ComponentResponder<OpensModal>` | declare `opens_modal` on the Route and take the `OpensModal` alias |
| `[INT-05]` responder alias doesn't match `opens_modal` (`` [INT-05] `opens_modal` is declared, so the handler must take `CommandResponder<OpensModal>` `` / `` [INT-05] this handler takes `CommandResponder<OpensModal>`, so its `#[command]` must declare `opens_modal` ``) | one declared without the other | declare both or neither |
| `` [INT-05] `opens_modal` handlers take `r: CommandResponder<OpensModal>` to send the modal `` | `#[command(.., opens_modal)]` on a handler with no responder parameter | add `r: CommandResponder<OpensModal>` and return `r.modal(m).await?.done()` |
| `modal` not found on `CommandResponder` (E0599) | the Command is not `opens_modal`, or takes `CommandResponder` | declare `opens_modal` and take `CommandResponder<OpensModal>` |
| `update` not found on `CommandResponder` | commands can't update | `reply` |

## App build errors (`BuildError`, all violations listed at once)

| Message | Fix |
| --- | --- |
| duplicate Route `x` in Modules A and B | rename one Route id (before release!) |
| Route `x` has no mounted handler | `.add(handler())` on a mounted Module |
| duplicate Command Path / pattern expansion | one handler per path (FR-56) |
| conflicting `default_member_permissions` (or contexts, integration types, nsfw) on base `x` | make leaves agree or declare `Module::base("x", …)` |
| base `x` has options and subcommands | split the paths |
| `Ext<T>` not provided | add a before-Middleware with `.provides::<T>()` on every path, or use `Option<Ext<T>>` |
| `State<T>` not registered | `.state(T)` on the builder |
| locale key defined by Modules A and B | rename one key |
| handler needs intent `GUILD_MEMBERS` | add `.intents(Intents::GUILD_MEMBERS)` and enable it in the portal |
| no font registered | enable `default-font` or `ImageConfig::font(..)` |
| animation name `spin` redefined | rename your keyframes |
| shard total must be at least 1 | remove `.shard_total(0)` / `NOCTIS_SHARD_TOTAL=0`, or set a positive total |
| invalid HTTP config: HttpConfig.`<field>` … is out of range | set the field inside the range the message names (or keep `HttpConfig::default()`) |
| the Discord HTTP edge could not bind 127.0.0.1:0 | the process may not open a loopback socket (sandbox, file-descriptor limit): allow loopback TCP |
| > 100 global chat-input commands | merge into fewer bases (groups) |

## Runtime

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| "This interaction failed" | handler panicked before the deadline on an `opens_modal` Route, or slow work before `r.modal(..)` | keep modal handlers fast (< 3 s); load data after opening if possible |
| "This interaction failed" / `AckDeadlineMissed` (`interaction not acknowledged in time: … past the 2.9 s ack deadline`) | the interaction waited in the queue past the deadline, so even the auto-defer failed (`WARN noctis::dispatch: auto-defer failed handler=..`), or an `opens_modal` handler was slow | Commands are auto-deferred at `ack_deadline` (2000 ms) and answer late by editing; keep `ack_deadline` ≤ 2000 ms, raise `.handler_concurrency(n)` if lanes back up. Check `noctis_ack_latency_seconds`, `noctis_http_phase_seconds{phase="ttfb"}` and `noctis_interaction_ack_hedges_total` for a slow network |
| `ERROR noctis::dispatch: an `opens_modal` handler did not answer within the 3 s ack deadline handler=.. module=..` | an `opens_modal` Command did slow work before `r.modal(..)` (no auto-defer for it) | open the modal first; load data after, or pre-load it cheaply |
| `ModalUnavailable` (`a modal can only be the first response to an interaction`) | `r.modal(..)` over the `Dispatcher` (prefix commands), or after another initial response | answer prefix invocations with a message; open modals only from interactions |
| the first Discord request after a quiet period is slow, or `RequestTimedOut` after ~10 s | a NAT/firewall silently dropped an idle connection, or slow DNS | nothing to add: the HTTP edge keeps one connection warm with HTTP/2 PINGs and redials dead ones; do **not** poll `GET /users/@me`. If it persists, lower `HttpConfig::ping_interval` and check `noctis_http_ping_timeouts_total` and `noctis_http_phase_seconds{phase="dns"}` |
| `ERROR noctis::http: the Discord HTTP edge could not start` | the tokio runtime has no IO driver | build it with `enable_all()` (as `#[tokio::main]` does) |
| clicking an old message shows "expired" | Ref TTL passed, or Payload schema changed (stale) | longer `ttl`, `on_expired`/`on_stale` handler that re-opens the View |
| clicks on another user's panel say "not allowed" | `owner = …` guard working as designed | make the View ephemeral or remove the guard |
| select shows old choice after update | missing `select` on the Route or no `default_*` set | add `select` and set defaults to the current value |
| mentions in a message don't ping | messages default to `parse: []` | opt in with `V2Message::mentions(..)` |
| `Violation { rule, path, limit, actual }`, printed as `[CV2-03] components: limit at most 40, actual 41` (`[RULE-ID] path: limit …, actual …`) | dynamic content exceeds a Limit | paginate the list yourself, sizing pages with `Budget` (`views-and-recipes.md` §3.2); shorten text; `LimitMode::Lenient` truncates text only |
| `[CV2-03] components: limit at most 40, actual 41` with notes `container[0]: 38` … | the tree has too many nodes (Section texts and accessories count) | paginate or split the message; see the per-node notes for the biggest subtree |
| `[CV2-04] components: limit at most 4000, actual 4210` | Text Displays together are too long | shorten text, paginate, or move long text into a modal or image |
| `[CV2-06] row[1]: limit between 1 and 5, actual 6` / `limit exactly one select, alone in its row, actual 2` | too many buttons in one row, or a select shares its row | split into several rows; give each select its own row |
| `` [CV2-09] row[0].select[0]: limit min_values ≤ max_values, actual `3 > 1` `` | `min_values` set without `max_values` (max defaults to 1) | set `max_values` too |
| `[CV2-09] row[0].select[0].options: limit between 1 and 25, actual 30` | too many (or no) String Select options | paginate the options or use an auto-populated select |
| `` [MOD-05] label[0].text_input: limit min_length ≤ max_length, actual `10 > 5` `` | Text Input bounds reversed | swap the bounds |
| `[ATT-01] attachments[0].size: limit at most 20971520, actual 31457280` (``= note: file `card.gif` ``) | one file over the per-file limit (or the interaction's `attachment_size_limit`) | render smaller (fewer frames, lower size) |
| `[ATT-02] request: limit at most 26214400, actual 27000000` | all files plus JSON over 25 MiB | send fewer files per message |
| `` [ATT-05] limit attachment://<name> of a file in this message, actual `attachment://card.png` `` | the URL names a file that is not attached (names are case-sensitive) | attach the file in the same message, or fix the name |
| `[INT-07] choices: limit at most 25, actual 40` (`= help: Noctis sends the first 25 choices and logs this as a warning`) | the autocomplete returned too many suggestions | filter or `take(25)` before `respond` |
| `Budget::fits` returns 0 | not even one row fits beside the fixed parts | shorten the row, render it as an image, or show it on a detail screen |
| `RenderError::Overloaded` | render queue full | show the busy notice; raise pool/queue in `ImageConfig`; cache more |
| `RenderError::CapExceeded` | too many pixels/frames/fps | smaller size, fewer frames, lower fps |
| card text is invisible / boxes for emoji | no font / no emoji font | `default-font`; `image-emoji` + `EmojiFont::Twemoji` |
| old card image after a design change | Template `version` not bumped | bump `version` |
| `TokenExpired` (`the interaction token expired (15 minutes)`) | ≥ 15 min since the interaction arrived: a follow-up, an ephemeral message, or an original that was deferred and never edited (no known id) | public messages with a known id fall back to bot-token edits and deletes automatically; ephemeral can't — send a new interaction (re-open) |
| `FollowupCap` (`a user-installed app may send at most 5 follow-ups here`) | user-installed app outside a guild (`authorizing_integration_owners` has only a user), > 5 follow-ups | combine into one View or edit the original instead |
| events missing under load | droppable events dropped by back-pressure | metrics `noctis_events_dropped_total`; make handlers tolerate gaps |
| `ERROR noctis::event: event overflow is full; event dropped` | a non-droppable event arrived while the event lane and its overflow were full | raise `.event_lane(capacity, overflow)` or `.handler_concurrency(n)`, speed up slow handlers; route drops with `.error_sink(ErrorOrigin::Event, ..)` |
| `ERROR noctis::app: shard closed fatally shard=.. close_code=..` | a shard stopped after startup with a fatal close code (or none); `error` names the cause (e.g. `4014 (disallowed intents)`); the App keeps running on the other shards | fix the cause (portal intents, token), then restart; watch `noctis_shard_status` |
| `run()` returns `RunError::AllShardsEnded` | every shard closed fatally after startup; the shutdown ran | read the `shard closed fatally` lines, fix the cause, restart |
| `ERROR noctis::event: event handler cancelled at shutdown` | a handler was still running (or queued behind an ordered one) when `shutdown_grace` ended | make it finish sooner, or raise `.shutdown_grace(..)` / `NOCTIS_SHUTDOWN_GRACE` |
| `WARN noctis::app: lanes discarded at shutdown` / `handlers still running after cancellation` | events were still queued at the deadline / a handler blocks its thread (no `.await`) | raise the grace; move blocking work to `spawn_blocking` |
| `run()` returns `StartError::SignalHandler` | the tokio runtime has no IO driver | build it with `enable_all()` (as `#[tokio::main]` does) |
| `ERROR noctis::error_sink: error sink panicked` | your `ErrorSink::report` panicked (caught; workers keep running) | fix the sink; never `unwrap` inside it |
| commands don't appear | not published | `cargo run --release -- --publish-guild <id>`; global publish can take time |
| shard closes with 4014 | privileged intent not enabled in the portal | enable it, or drop the handler that needs it |
| `run()` returns `StartError::DisallowedIntents { shard }` | close 4014 during startup: a privileged intent is not enabled in the portal | enable it, or stop requesting it |
| `run()` returns `StartError::InvalidToken` | `GET /gateway/bot` answered 401, or close 4004 | copy the token again from the Developer Portal |
| `run()` returns `StartError::ShardTotalTooSmall { total, minimum }` | a fixed shard total below ⌈recommended × 2 / 5⌉ (GW-03) | remove `shard_total` or raise it to at least `minimum` |
| `run()` returns `StartError::SessionStartLimit { .. }` | the daily identify budget can't cover every shard (GW-02) | wait `reset_after`, or run fewer shards |
| warning "shard total is not a multiple of max_concurrency" | a fixed total misaligned with Discord's identify buckets | use a multiple of `max_concurrency`, or ignore it (startup is only slower) |
| startup warning "Cluster without HTTP proxy" | several processes, no `NOCTIS_HTTP_PROXY` | configure a shared proxy |
| two processes disagree on module state | different `ModuleStateStore`s | share one Adapter; propagation ≤ poll interval |
| Ref minted on one process "expired" on another | memory `RefStore` in a Cluster | shared `RefStore` Adapter |

## Where to look

1. The error's `help:` line (always actionable).
2. `limits-cheatsheet.md` for the rule id.
3. The topic reference (`commands.md`, `components-v2.md`, …).
4. If still unclear: report that the Noctis skill doesn't cover it. Do not read the
   framework source.
