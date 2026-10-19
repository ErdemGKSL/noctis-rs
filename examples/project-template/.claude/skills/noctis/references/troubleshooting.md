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
| `[CID-01] route \`x\` has unbounded fields` | `String`/`Vec` in an inline Route | `by_ref` (or `overflow = spill`), or store the data via a port and carry an id; `BoundedStr<N>` only for tiny ASCII |
| `[CID-01] route \`x\` can exceed 100 characters` | too many/wide fields, long id | shorten the Route id, use `PayloadEnum`/smaller ints, or `by_ref`; run the generated width test to see the breakdown |
| `[CID-03] invalid route id` | uppercase, spaces, `:`, > 24 chars, leading `_` | `[a-z0-9._-]{1,24}` |
| `by_ref` with `signed` rejected | a Ref is already unguessable | drop `signed` |
| `the trait Handled is not implemented for X` | building a component from a Route with no `#[handler]` | write the handler (and `.add(it())` to a Module) |
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
| `[CV2-05] Container cannot contain Container` / type mismatch on `.push` | illegal nesting | flatten; one Container per View |
| `[CV2-06]` unsatisfied bound in `ActionRow::buttons` | 0 or > 5 buttons, or button + select | split rows; selects alone |
| `[CV2-07]` Section arity / no accessory | 0 or > 3 texts | merge texts; use plain Text if no accessory |
| `[CV2-08] label too long` | literal > 80 | shorten (guideline ≤ 34 with emoji) |
| `[MOD-02] more than 5 fields` | modal too big | split into a Wizard with two modal Steps |
| `[MOD-07]` radio options | enum with < 2 or > 10 variants | adjust the `Choices` enum |
| unknown Tailwind class `bg-gradient-to-r`, help: `bg-linear-to-r` | v3 name | use the v4 name (see `images.md` §4) |
| unknown class `bg-brand` | token missing from `noctis-theme.css` | add `--color-brand` under `@theme` |
| `[LOC-01] unknown locale file` | file name is not exactly a Discord locale code (`en.yaml` is not one) | rename (`tr.yaml`, `pt-BR.yaml`, `en-US.yaml`) |
| missing key / wrong number of arguments on `s.x.y(..)` | key absent in `en-US.yaml` or placeholders differ | add the key; match placeholders |
| `HashMap` field in a Template | non-deterministic cache key | `BTreeMap` or `Vec` |
| a handler taking a Responder doesn't return `Responded` | macro contract | return `acked.done()` |
| `modal` not found on `ComponentResponder` | Route not `opens_modal`, or the handler takes `ComponentResponder` instead of `ComponentResponder<OpensModal>` | declare `opens_modal` on the Route and take the `OpensModal` alias |
| `[INT-05]` responder alias doesn't match `opens_modal` | one declared without the other | declare both or neither |
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
| fixed shard total too small | remove `shard_total` or raise it |
| > 100 global chat-input commands | merge into fewer bases (groups) |

## Runtime

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| "This interaction failed" | handler panicked before the deadline on an `opens_modal` Route, or slow work before `r.modal(..)` | keep modal handlers fast (< 3 s); load data after opening if possible |
| clicking an old message shows "expired" | Ref TTL passed, or Payload schema changed (stale) | longer `ttl`, `on_expired`/`on_stale` handler that re-opens the View |
| clicks on another user's panel say "not allowed" | `owner = …` guard working as designed | make the View ephemeral or remove the guard |
| select shows old choice after update | missing `select` on the Route or no `default_*` set | add `select` and set defaults to the current value |
| mentions in a View don't ping | Views default to `parse: []` | override `View::mentions` |
| `Violation { rule, path, limit, actual }`, printed as `[CV2-03] components: limit at most 40, actual 41` (`[RULE-ID] path: limit …, actual …`) | dynamic content exceeds a Limit | let the Paginator handle lists; shorten text; `LimitMode::Lenient` truncates text only |
| `UiError::ItemTooLarge` | a single list item can't fit beside the Chrome | render it as an image or a detail View |
| `UiError::TooManyTabs` | > 5 Navbar tabs | select-based nav or split Views |
| `RenderError::Overloaded` | render queue full | show the busy notice; raise pool/queue in `ImageConfig`; cache more |
| `RenderError::CapExceeded` | too many pixels/frames/fps | smaller size, fewer frames, lower fps |
| card text is invisible / boxes for emoji | no font / no emoji font | `default-font`; `image-emoji` + `EmojiFont::Twemoji` |
| old card image after a design change | Template `version` not bumped | bump `version` |
| `TokenExpired` | > 15 min since the interaction | public Views fall back to bot-token edits automatically; ephemeral can't — re-open |
| `FollowupCap` | user-installed app outside a guild, > 5 follow-ups | combine into one View |
| events missing under load | droppable events dropped by back-pressure | metrics `noctis_events_dropped_total`; make handlers tolerate gaps |
| commands don't appear | not published | `cargo run --release -- --publish-guild <id>`; global publish can take time |
| shard closes with 4014 | privileged intent not enabled in the portal | enable it, or drop the handler that needs it |
| startup warning "Cluster without HTTP proxy" | several processes, no `NOCTIS_HTTP_PROXY` | configure a shared proxy |
| two processes disagree on module state | different `ModuleStateStore`s | share one Adapter; propagation ≤ poll interval |
| Ref minted on one process "expired" on another | memory `RefStore` in a Cluster | shared `RefStore` Adapter |

## Where to look

1. The error's `help:` line (always actionable).
2. `limits-cheatsheet.md` for the rule id.
3. The topic reference (`commands.md`, `components-v2.md`, …).
4. If still unclear: report that the Noctis skill doesn't cover it. Do not read the
   framework source.
