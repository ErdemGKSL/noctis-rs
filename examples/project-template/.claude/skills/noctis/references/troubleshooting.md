# Troubleshooting

Find the symptom, apply the fix. Errors name a rule id (`[CID-01]`); look it up in the
`## Rule id index` at the end of this file (meaning and fix) and in `limits-cheatsheet.md`
(numbers). For mistakes that compile but misbehave, see `patterns.md` (`## Common
mistakes`). Never work around a check by reading or patching Noctis source;
if the skill doesn't explain an error, say so.

Build, test and run with `--release` only (`cargo build --release`, `cargo test
--release`, `cargo clippy --release --all-targets -- -D warnings`, `cargo run
--release`). If the disk fills, delete `target/debug/`.

## Compile errors

| Error | Cause | Fix |
| --- | --- | --- |
| ``[CID-01] route `x` has unbounded fields`` (`` = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to the kind attribute ``) | `String`/`Vec` in an inline Route | `by_ref` (or `overflow = spill`), or store the data via a port and carry an id; `BoundedStr<N>` only for tiny ASCII |
| ``[CID-01] route `x` can exceed 100 characters: worst case 101`` (`` = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown ``) | too many/wide fields, long id | shorten the Route id, use `PayloadEnum`/smaller ints, or `by_ref`; `cargo test --release -- --nocapture noctis_route_width` prints the breakdown |
| ``[CID-01] `f32` is not a Payload field type`` | a field type without `Field` | use a built-in field, implement `Field` + `FieldDecode`, or derive `PayloadEnum` |
| `[CID-01] a Payload cannot be generic` (also `a PayloadEnum`); `` `#[derive(Button)]` needs a struct with named fields or a unit struct `` (any kind derive) | type, lifetime or const parameters, a `where` clause, a tuple struct, an enum or a union | make the struct concrete, with named fields or none |
| `` no method named `update` found for struct `CommandCtx` `` (E0599) | `ctx.update(..)` in a Command handler: a Command has no message to update (INT-04) | `ctx.respond(msg)` (a new message, or the edit of the loading message after the auto-defer) |
| ``[CID-02] duplicate custom_id: `Press { n : 1 }` is already used at row[0].button[0]`` (`` = help: give the components a Payload field that tells them apart, … ``) | two literal components with the same Route and Payload in one `components!` | give the Route a Payload field that differs between them (prev/next carry their target page, per-row buttons the row id) |
| `[CID-01] a Payload must be a struct, not an enum` (or `not a union`) | `#[derive(Payload)]` on an enum | a struct; `PayloadEnum` for unit enums |
| `[CID-01] a PayloadEnum has at most 62 variants (one base62 digit)` / ``PayloadEnum variant `B` has fields`` / `a PayloadEnum needs at least one variant` / `PayloadEnum must be derived on an enum` | variant 63, a tuple/struct variant, an empty enum, or a struct | split the enum or use an integer field; unit variants only |
| ``[CID-03] route id: limit [a-z0-9._-], actual `Shop` `` with ``= note: `S` at byte 0 is not allowed`` | uppercase, spaces, `:`, `,`, `~`, `!`, non-ASCII | `[a-z0-9._-]` |
| `[CID-03] route id: limit between 1 and 24, actual 25` | empty or too long id | 1 to 24 characters |
| ``[CID-03] route id: limit an id that does not start with `_`, actual `_x` `` | `_` prefix is reserved for Noctis | rename |
| `[CID-03] #[derive(Button)] needs a #[button("id", defaults(..))] attribute` / `a struct takes exactly one #[button] attribute` / `#[button] takes a list: the id, then codec arguments and `defaults(..)`` / `#[button] is missing the route id` / `the route id must be a string literal` (each kind names its own attribute) | attribute missing, doubled or malformed | `#[button("shop.buy", args…, defaults(..))]` |
| ``[CID-03] unknown #[button] argument `colour` `` (also `select` on a select kind, `view`, `opens_modal`) | typo, unsupported or withdrawn argument | codec arguments: `by_ref`, `overflow = spill`, `schema = n`, `signed`, plus `defaults(..)`; every select kind carries the nonce itself; `view` is not an argument (tabs and pages are Payload fields) |
| ``[INT-14] the rate-limit count must be an integer literal in 1..=4294967295`` / ``[INT-14] the rate-limit window must be 1s..=1d; found "2d"`` / ``[INT-14] `per` takes a duration such as "10s", "1m" or "1h30m" (units d, h, m, s)`` / ``[INT-14] `rate_limit` needs a window: `rate_limit(user, 3, per = "10s")` `` / ``[INT-14] `rate_limit` takes `(scope, count, per = "…"[, manual])` `` | a malformed `rate_limit(..)` | `rate_limit(user, 3, per = "10s")`; count 1..=4294967295, window 1 s..=1 d, optional trailing `manual` |
| ``[INT-14] unknown rate-limit scope `users` …`` / ``[INT-14] a second `rate_limit` for scope `user`: declare each scope once`` / ``[INT-14] scope `message` needs a message: use it on #[message_command], a component or a modal`` | wrong scope name, a scope declared twice, or `message` on `#[command]`/`#[user_command]` | scopes `user`, `member`, `channel`, `guild`, `message`; one limit per scope |
| E0277 ``[INT-14] #[on_rate_limited] needs a Command, context-menu, component or modal struct`` | `#[on_rate_limited]` on an `EventHandler` or `Hook` struct | remove it: events and Lifecycle Hooks are never rate-limited |
| E0277 `` `X` is not a lifecycle event`` / ``lifecycle hooks take (self, ctx: HookCtx, event: <lifecycle event>)`` / ``lifecycle hooks run no Middleware ..`` | a `#[derive(Hook)]` handler with a gateway payload or another type, a missing or extra parameter, a responder, or an `Ext<T>` | `async fn handle(self, ctx: HookCtx, event: <a noctis::lifecycle type>) -> HandlerResult<()>` (`app-and-sharding.md` §7) |
| `` `#[derive(Hook)]` needs a unit struct`` / ``unknown `#[hook]` argument `ordered`: expected `flags = ["beta"]` `` | fields, generics or an enum; an argument other than Boot Flags | `#[derive(Hook)] pub struct X;` with at most `#[hook(flags = [..])]` |
| `RateLimitError::Undeclared { scope }` / `Unkeyed { scope }` from `ctx.rate_limits()` | the handler declares no `rate_limit` of that scope / the interaction has no key for it (no channel; a `/`-opened modal for `message`) | declare the scope on the kind attribute; do not use `message` where no message exists |
| E0277 `` [INT-05] `auto_defer = false` is declared, so the handler must take the `Manual` responder `` / `` [INT-05] this handler takes a `Manual` responder, so its attribute must declare `auto_defer = false` `` / `` [INT-05] `auto_defer = false` handlers take a `Manual` responder `` on a `#[modal]` handler or hook | `#[modal(.., auto_defer = false)]` and the handler's (or its `#[on_stale]`/`#[on_expired]` hook's) responder disagree | take `ModalResponder<Manual>` exactly when the modal declares `auto_defer = false`; otherwise take `ModalResponder` or none |
| ``[CID-03] `owner` takes a field name such as `owner = user` `` / ``[CID-03] `guard` takes a function path such as `guard = auth::is_staff` `` | a Route Guard with no value, a string, a field path (`a.b`), a number, or generic arguments (`f::<T>`) | `owner = <Payload field>`, `guard = path::to::fn` |
| ``[CID-03] `owner = usr` names no field of `S`; its fields are: user, page`` (or `it has no fields`) / ``[CID-03] `owner` must name a Payload field; `bio` is a modal input`` | a typo, a unit struct, or a modal input as the owner | name an `Id<UserMarker>` Payload field (add one to the struct) |
| E0277 `` `owner` needs a field of type `Id<UserMarker>`, found `Option<Id<UserMarker>>` `` (or `u64`, `Id<GuildMarker>`) | the `owner` field is not an `Id<UserMarker>` | make the field `Id<UserMarker>`; an optional owner is not supported: use a custom `guard = path` |
| E0308 at a `guard = path` (`expected &ModalSubmitCtx, found &ComponentCtx`, or `expected Result<Access, HandlerError>`) | the custom Guard's signature does not fit the Route | `async fn(ctx: &ComponentCtx, this: &Self) -> HandlerResult<Access>` (`&ModalSubmitCtx` on `#[modal]`) |
| ``[CID-03] `ttl` needs a by-Ref Route`` / ``[CID-03] `one_shot` needs a by-Ref Route`` | `ttl`/`one_shot` on an inline Route: only a Ref has a TTL or is consumed on use | add `by_ref` or `overflow = spill`, or drop the argument |
| ``[CID-03] `ttl = "15x"` is not a duration`` / ``[CID-03] `ttl` must be between 1s and 365d, found `366d` `` / ``[CID-03] `ttl` takes a duration string`` | a `ttl` outside the grammar or the range | `<digits><unit>` groups in the order `d`, `h`, `m`, `s` (`"15m"`, `"1h30m"`, `"2d"`), 1 s..=365 d |
| `ref TTL … is out of range: use a value in 1 s..=365 d` (`BuildViolation::InvalidRefTtl`) | `.ref_ttl(..)` below 1 s or above 365 d | a value in 1 s..=365 d |
| ``[CID-03] #[button] argument `signed` is given twice`` / ``[CID-03] `signed` is a flag and takes no value`` / ``malformed #[button] argument `by_ref` `` | duplicate argument, `signed = true`, `by_ref()` | write each flag once, bare |
| ``[CID-03] `overflow = error` is not a #[button] argument`` (`` = help: the plain inline Route (no `overflow`) is the error policy: leave `overflow` out ``) / ``the only `overflow` policy is `spill` `` / ``[CID-03] `overflow` needs a value`` | another overflow policy | `overflow = spill`, or nothing |
| ``[CID-03] `schema` must be in 1..=61 (one base62 digit), found `62` `` / ``[CID-03] `schema = 0` is the default schema`` (`` = help: leave it out for schema 0 ``) / ``[CID-03] `schema` must be an integer literal`` / ``[CID-03] `schema` needs a value`` | schema out of range or not an integer literal | `schema = 1` … `schema = 61`; no `schema` for 0 |
| ``[CID-03] `by_ref` and `overflow = spill` cannot be combined`` | both body policies | pick one |
| `DecodeError::Malformed(CodecError::ReservedFlag)` (`format digit sets the reserved flag 2`) in a log | a `custom_id` whose format digit sets the reserved flag bit 2: not written by this Noctis (a forged id or one from an old build) | nothing to fix in the bot; Noctis rejects it as malformed |
| ``[CID-03] a `by_ref` route cannot be `signed` `` | a Ref is already unguessable | drop `signed` |
| conflicting implementations of `Payload` | a kind derive and `#[derive(Payload)]` on one struct | keep only the kind derive; it includes the Payload |
| E0080 ``[CID-04] `X` has no #[handler] method, so no component can be built for it`` | a component or modal struct without a `#[handler]` method | write the `#[handler]` method (and `.add::<X>()` to a Module) |
| E0080 `` `X` has no #[handler] method, so it cannot be registered `` | a command, context-menu or event struct without a `#[handler]` method | write `impl X { #[handler] async fn handle(self, ctx, ..) }` |
| ``write `Route { .. }.build()` `` (in `components!` or as a method error) | a withdrawn per-kind constructor method | `X { .. }.build()` |
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
| `[OPT-11]` autocomplete flag without a method, method without a flag or for an unknown field, or `Suggestions<T>` of the wrong type | the `autocomplete` flag and the `#[autocomplete(field)]` method must pair up, with `T` = the field's type | add the missing half; return `Suggestions<i64>` for an `i64` field (`commands.md` §4) |
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
| `[INT-05] show_modal needs auto_defer = false on this handler` (E0277) | `show_modal` on an auto-defer responder (`CommandResponder`, `ComponentResponder`) or on any `ModalResponder` | declare `auto_defer = false` on the command or component struct and take `CommandResponder<Manual>` / `ComponentResponder<Manual>`; a modal submit can never open a modal |
| `[INT-05]` responder alias doesn't match `auto_defer` (`auto_defer = false` declared but the handler takes `CommandResponder`/`ComponentResponder` or no responder, or it takes the `Manual` alias without `auto_defer = false`) | one declared without the other | declare both or neither. Messages: `` [INT-05] `auto_defer = false` is declared, so the handler must take the `Manual` responder (`CommandResponder<Manual>`, `ComponentResponder<Manual>` or `ModalResponder<Manual>`) `` / `` [INT-05] this handler takes a `Manual` responder, so its attribute must declare `auto_defer = false` `` / `` [INT-05] `auto_defer = false` handlers take a `Manual` responder `` |
| ``[INT-04] a Command cannot `update` its message: write `response = public` or `response = ephemeral` `` / `` `response` takes `public` or `ephemeral` `` / `` `auto_defer` takes `true` or `false` `` | `response = update` (or another value) on a command or context menu; a non-bool `auto_defer` | `response = public` or `response = ephemeral`; `auto_defer = true` or `false` |
| ``[CID-03] `response` takes `update`, `ephemeral` or `public` `` / ``[CID-03] #[button] argument `response` is given twice`` | a quoted or unknown `response` value, or a repeated argument, on a component attribute | write a bare `response = update`, `ephemeral` or `public`, once |
| `the parameters of this #[handler] method do not fit ..` on a component or modal | it takes another kind's responder (`CommandResponder` on a button, `ComponentResponder` on a modal) | `ComponentResponder` for buttons and selects, `ModalResponder` for modals |
| ``unknown `#[command]` argument `ephemeral`: write `response = ephemeral` `` (also on `#[user_command]`/`#[message_command]`, and the old modal flag, whose message says `write auto_defer = false and take CommandResponder<Manual>`) | a withdrawn bare flag | write the replacement the message names |
| ``[INT-06] autocomplete cannot be deferred: `#[autocomplete]` takes no `auto_defer` or `response` `` | `auto_defer`/`response` on `#[autocomplete]`: autocomplete has no defer | remove it; finish within 2.5 s |
| a free `async fn` with `#[handler]`/`#[command]`/`#[user_command]`/`#[message_command]`/`#[event]`, or one of the withdrawn derives (help shows the struct form) | the old free-function style | one struct per interaction: `#[derive(SlashCommand/Button/…)] struct X` + `impl X { #[handler] async fn handle(self, ctx) }` |
| `.build()` not found, help lists the kind derives | a struct without a kind derive (`Button`, `StringSelectMenu`, …) | derive the kind and add `defaults(..)` |
| ``#[button]` needs `defaults(..)` with `style` and `label` or `emoji` `` / ``… with `placeholder` `` / ``#[modal]` needs `defaults(title = "…")` `` | the mandatory defaults of the kind | add the keys the error names |
| E0277 ``[CV2-09] this String Select has no options: call `.options(..)` or declare `defaults(options = …)` `` | a `StringSelectMenu` with no `options` in its defaults, placed without `.options(..)` | call `.options(..)` per render or add `options = SomeChoicesEnum` |
| `update` not found on `CommandResponder` | commands can't update | `reply` |
| E0277 ``[WH-02] `V2Message` may hold interactive components: a webhook the app does not own sends only a `StaticMessage` `` | `foreign(..)?.send(v2_message)` / `.edit(..)` | build it with `StaticComponents`/`StaticContainer`/`StaticSection`/`StaticActionRow` (`components-v2.md` §12b), convert a runtime tree with `StaticMessage::try_from(msg)?`, or use an owned webhook (`create`/`owned`) for interactive Views |
| E0277 ``[WH-02] `RouteButton<X>` is not a static row button`` / ``… is not a static Section accessory`` / ``[WH-02] `ActionRow` cannot be a top-level component of a StaticMessage`` (or ``a StaticContainer child``) | a Route button, premium button, select, or a plain `ActionRow`/`Section`/`Container` in a static tree | only `LinkButton`s and Thumbnails in static rows and accessories; use the `Static*` builders, not the V2 ones |
| `` no method named `with_components` found for struct `ExecuteWebhook` `` (E0599) | trying to turn off `with_components` (WH-01) | nothing to do: Noctis always sends `with_components=true` |
| ``[WH-04] name: limit between 1 and 80, actual 81`` / ``[WH-04] name: limit a name without `clyde` or `discord` (any case), actual `Discord log` `` | a `webhook_name!` literal that is empty, too long, or contains a reserved word | 1–80 characters without `clyde`/`discord`; `WebhookName::try_new(text, &limits)` for runtime names |
| ``[TW-01] unknown Tailwind class `bg-gradient-to-r`; use `bg-linear-to-r` (Tailwind v4)`` / ``…; or declare `--color-nope` in noctis-theme.css`` | a v3 or misspelt class in `tw!`/`tw="…"`, or a theme token the crate's `noctis-theme.css` does not define | use the v4 name the error suggests, or declare the token in `noctis-theme.css` (`images.md` §4-§5) |
| ``[TW-02] 3:5: …`` (from `tw!`, `view!` or `theme!()`, with `= note: in …/noctis-theme.css`) | the theme file breaks a rule (`ThemeErrorKind`) at that line:column | fix that line (`images.md` §5); `spin`/`ping`/`pulse`/`bounce` are built in and cannot be redefined |
| ``Template data must serialize deterministically (it is part of the render cache key): `HashMap` …`` | a `HashMap`/`HashSet` in a `#[derive(Template)]` field | use `BTreeMap`, `BTreeSet` or `Vec` |
| ``[IMG-03] `#[asset]` fields are `RemoteImage`, `Option<RemoteImage>` or `Vec<RemoteImage>` `` | an `#[asset]` field of another type | make it a `RemoteImage` (or `Option`/`Vec` of one) built with `RemoteImage::avatar`/`guild_icon`/`url`/`placeholder` (`images.md` §7) |
| ``[IMG-03] remote origin `<s>` is invalid: …`` / ``[IMG-03] remote <name> <v> is out of range: use <range>`` (plugin violations) | a bad `RemoteConfig::allow_origin`/`discord_cdn` origin, or a limit outside its range | an origin is `https://host[:port]` (plain `http` only for loopback hosts), no path or wildcard; keep each limit in the range the message names |
| ``[IMG-03] template `<id>` requires remote image `<image>`, which could not be used: <reason>`` (`RenderError::RemoteImage`) | an `#[asset(required)]` image failed (status, timeout, cap, format, not allow-listed) | drop `required` to draw the fallback, allow-list the origin, or raise the cap the reason names |
| ``[IMG-03] template `<id>` draws remote image `<image>`, which no `#[asset]` field declares`` (`RenderError::UndeclaredImage`) | the view draws a `RemoteImage` that is not in an `#[asset]` field | put the image in an `#[asset]` field and draw that field |
| `remote image unavailable; drawing the fallback` (warning, target `noctis::image`) | a non-required remote image failed; `reason` says why | allow-list the origin (`RemoteConfig::allow_origin`), or accept the fallback |
| ``WARN noctis::image: [IMG-04] the `image-emoji` feature is off: Unicode emoji in Template text render as boxes`` | the App registers a Template without the `image-emoji` feature | enable `image-emoji` (or ignore it when Templates never show Unicode emoji) (`images.md` §6) |
| ``[IMG-04] custom emoji in literal `view!` text are never fetched: …`` (compile error) | a `<:name:id>` token inside a string literal in `view!` | put the text in a Template field (data is scanned), or declare `#[asset] e: RemoteImage` with `RemoteImage::custom_emoji(id, animated)` and draw `<img src={&self.e} />` (`images.md` §7) |
| ``[IMG-03] template `<id>` draws remote image `emoji/<n>`, which no `#[asset]` field declares`` | the view builds a custom emoji token itself (`format!`) that is not in the Template's data | pass the text through a data field, or declare the emoji with `RemoteImage::custom_emoji` |
| ``WARN noctis::image: <n> custom emoji over the per-job cap of <max> draw the fallback`` / ``[IMG-04] remote max_emoji <v> is out of range: use 0..=128`` | more distinct custom emoji in one job's data than `RemoteConfig::max_emoji` (default 32) / a cap outside 0..=128 | raise `max_emoji` (at most 128), or accept the fallback for the rest |
| ``invalid Boot Flag `Beta` in `flags`: `B` at index 0 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-` `` (also `` `all` is reserved … ``, `` a Boot Flag is given twice: keep one ``, `` the Boot Flag has 33 characters: shorten it to at most 32 ``) | a `flags = [..]` literal on a kind attribute breaks the Boot Flag grammar | use 1-32 characters of `a-z`, `0-9`, `_`, `-`, never `all`/`none`, each once (`modules-middleware-ratelimits.md` §2) |
| `` `flags` needs at least one Boot Flag: write `flags = ["beta"]` or remove it `` / `` `flags` takes a list of string literals: write `flags = ["beta"]` `` | `flags = []`, `flags = "beta"`, `flags = [beta]` or a bare `flags` | write `flags = ["beta"]`, or drop the argument for an unflagged registration |
| ``[IMG-01] no font is registered: add one with `ImageConfig::font(..)` or enable the `default-font` feature`` (a `TemplateRejected` build error) | the App registers a Template but no font, and `default-font` is off | enable `default-font`, or `.image(ImageConfig::new().font(Font::from_static(include_bytes!(..))))` (`images.md` §6) |
| ``[IMG-01] font #<i> …`` (plugin violation) | the `<i>`-th `.font(..)` (from 0) has an inverted `unicode_range`, a bad `family` name, or bytes that are not TTF/OTF/TTC/WOFF2 | fix that font |
| `ResourceConflict` naming `FontRegistry` | two Apps on one `Runtime` register different fonts (or a different order, or none) | give every App the same font list, or a separate `Runtime` |
| ``[IMG-02] static image id must match …`` (compile error in a `const`) | a `StaticImage::new` id with uppercase, spaces, a leading/trailing `.`/`_`/`-`, or over 64 bytes | use a lowercase id such as `logo.v2` |
| ``[IMG-02] static image `<id>` …`` (plugin violation) | the image is not PNG/JPEG/WebP, is corrupt, is over 16 MP, or its id is registered twice with different bytes | convert or shrink it, or rename one id |
| `RenderError::UnknownImage { id, image }` | the Template drew a `StaticImage` the App did not pass to `ImageConfig::image` | register it with `.image(LOGO)` |
| ``` `<svg>` is not supported …``` / ``` `<img>` needs `src={..}` …``` / ``` `<img>` has no children …``` | `view!` `<img>` grammar | write `<img src={LOGO} tw=".." />` with a PNG/JPEG/WebP `StaticImage` |

## App build errors (`BuildError`, all violations listed at once)

| Message | Fix |
| --- | --- |
| ``[CID-06] Route `{route}` is `signed`, but no payload key is configured: set NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)` `` | `BuildViolation::MissingPayloadKey`: a mounted `signed` Route needs the key | set `NOCTIS_PAYLOAD_KEY` (`openssl rand -hex 32`; the template writes one on first run) or call `.payload_key(..)` |
| ``[CID-06] a previous payload key is set without a current one: set NOCTIS_PAYLOAD_KEY (or call `.payload_key(..)`) too`` | `BuildViolation::PreviousKeyWithoutKey` | set the current key as well; the previous one only verifies |
| ``environment variable `NOCTIS_PAYLOAD_KEY` is not a payload key: set 64 hex digits, such as the output of `openssl rand -hex 32` `` | `EnvError::InvalidPayloadKey` (a base64 or truncated value; the source names the length or the first non-hex position) | replace it with 64 hex digits |
| ``[CID-04] Route `{route}` has two handlers: `{first_handler}` (Module `{first_module}`) and `{second_handler}` (Module `{second_module}`): keep one handler per Route id and rename the other Route (renaming breaks its live components)`` | `BuildViolation::DuplicateRoute`: `.add::<X>()` in one mounted Module only; for two different structs, rename one Route id (before release!) |
| ``Template id `{id}` is registered twice: `{first_type}` (Module `{first_module}`) and `{second_type}` (Module `{second_module}`): give each Template its own id`` | `BuildViolation::DuplicateTemplate`: `.template::<X>()` once per App, and a distinct `#[template(id = ..)]` per Template |
| ``invalid Module id `{id}`: {reason}`` | `BuildViolation::InvalidModuleId` with a `ModuleIdError`: the id breaks the grammar (`Empty`, `TooLong`, `InvalidChar`, `EmptySegment`: segments of `a-z`, `0-9`, `_`, `-` joined by one `/`, 1-64 characters) or its tree position (`PathAtRoot`: a root id is one segment, so nest it with `.module(..)`; `NotUnderParent`: a child is `<parent>/<name>`): rename the Module as the reason says (`modules-middleware-ratelimits.md` §1, Module trees) |
| ``invalid Boot Flag `{flag}` on Module `{module}`: {reason}`` | `BuildViolation::InvalidBootFlag`: a `Module::flags` entry breaks the grammar or repeats a flag of that Module: fix or drop it (`modules-middleware-ratelimits.md` §2) |
| ``Module `{module}` restricts to App id `{id}`: {reason}`` | `BuildViolation::InvalidAppRestriction`: a `Module::apps` id breaks the App id grammar `[a-z0-9-]{1,32}` (the reason is an `AppIdError`): fix it (`app-and-sharding.md` §10) |
| ``invalid App id `{id}` in `apps`: {reason}`` (compile error) | an `apps = [..]` id breaks `[a-z0-9-]{1,32}`: lowercase it, drop the invalid character or shorten it |
| an `apps = ["main"]` Command is "unknown command" or missing from Publish | the App's effective id is not listed: set `.app_id("main")` explicitly (without it the id is the token's or `default`) |
| a guild gets "This feature is disabled here." for every command | the App's `GuildAccessList` refuses that guild (`GuildAccess::allow` without it, or `deny` with it); DMs are always served |
| ``Boot Flag `{flag}` is selected, but no mounted Module or registration declares it: declared flags are `a`, `b` `` (or `declared flags are none`) | `BuildViolation::UnknownBootFlag`: a typo in `--features`, `NOCTIS_FEATURES` or `.boot_flags(..)`, or the flag's Module is not mounted: select a declared flag |
| ``Module id `{id}` is mounted twice: give each Module its own id (a Module tree is mounted at most once per App)`` | `BuildViolation::DuplicateModule`: two mounted Modules (any trees, plugins included) share an id, or one tree is mounted twice on the App: give each Module its own id, or mount the tree once |
| ``[CID-04] Module `{module}` uses Route `{route}` (`{route_type}`), but no mounted Module handles it: add `.add::<{route_type}>()` to a mounted Module`` | `BuildViolation::UnmountedRoute`: `.add::<X>()` on a mounted Module, or mount the Module that adds it |
| duplicate Command Path / pattern expansion | one handler per path (FR-56) |
| conflicting `default_member_permissions` (or contexts, integration types, nsfw) on base `x` | make leaves agree or declare `Module::base("x", …)` |
| base `x` has options and subcommands | split the paths |
| ``handler `{handler}` (Module `{module}`) takes `Ext<{T}>`, but no before-Middleware on its Module path provides it: add `.before(Middleware::before(f).provides::<{T}>())` to Module `{module}` or an ancestor, or take `Option<Ext<{T}>>` `` | `BuildViolation::MissingExtension`: register the providing Middleware on the handler's Module or an ancestor (a sibling or child does not count), with a scope covering the handler (`events_only` never covers a Command, `skip_events` never an event), or take `Option<Ext<T>>` |
| `State<T>` not registered | `.state(T)` on the builder |
| locale key defined by Modules A and B | rename one key |
| handler needs intent `GUILD_MEMBERS` | add `.intents(Intents::GUILD_MEMBERS)` and enable it in the portal |
| ``[GW-05] event handler `noctis_prefix::PrefixPlugin` (Module `noctis-prefix`) handles MESSAGE_CREATE, which needs the privileged intent MESSAGE_CONTENT …`` | `PrefixPlugin` reads message text: add `.intents(Intents::MESSAGE_CONTENT)` and enable Message Content in the Developer Portal (`commands.md` §9) |
| `` plugin `noctis_prefix::PrefixPlugin`: prefix is empty `` / `prefix contains whitespace` / `prefix is longer than 32 characters` | a prefix of 1 to 32 Unicode scalars without whitespace (`"!"`, `"?"`, `"ş>"`) |
| `` plugin `noctis_prefix::PrefixPlugin`: no Command is opted in: add `.command::<T>()` `` | opt at least one `#[derive(SlashCommand)]` struct in |
| `` `bot::Ban` cannot be a prefix command: it declares `permissions` `` (or `nsfw = true`, or ``its option `x` declares `channel_types` ``), `which a message cannot enforce` | do not opt that Command in; prefix commands cannot check member permissions, age gates or channel types |
| `` `bot::Secret` cannot be a prefix command: it declares `response = ephemeral`, which a message cannot enforce `` | drop `response = ephemeral` or do not opt that Command in; a prefix command answers with a public message reply |
| ``Command Path `warn` is opted in by both `a::Warn` and `b::Warn` `` | opt in only one of the two types |
| ``[GW-05] Discord cache resource MEMBER needs the privileged intent GUILD_MEMBERS: add `.intents(Intents::GUILD_MEMBERS)` and enable it in the Discord Developer Portal, or remove MEMBER from `.discord_cache(..)` `` | `BuildViolation::MissingCacheIntent`: request the intent (and enable it in the portal), or drop `MEMBER` from `.discord_cache(..)` (`adapters.md` §1.2) |
| no font registered | enable `default-font` or `ImageConfig::font(..)` |
| animation name `spin` redefined | rename your keyframes |
| shard total must be at least 1 | remove `.shard_total(0)` / `NOCTIS_SHARD_TOTAL=0`, or set a positive total |
| invalid HTTP config: HttpConfig.`<field>` … is out of range | set the field inside the range the message names (or keep `HttpConfig::default()`) |
| the Discord HTTP edge could not bind 127.0.0.1:0 | the process may not open a loopback socket (sandbox, file-descriptor limit): allow loopback TCP |
| > 100 global chat-input commands | merge into fewer bases (groups) |
| `module state poll interval 9ms is out of range: use a value in 10 ms..=1 h` (`BuildViolation::InvalidModuleStatePoll`) | pass `.module_state_poll(..)` a value in 10 ms..=1 h (default 5 s) |

## Runtime

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `WebhookError::Invalid` with `[WH-05] thread_id: …` | a webhook send into a forum or media channel without `.thread(..)`, or `WebhookThread::Create` in a text channel (also mapped from Discord's codes 220001/220003) | pass `.thread(existing_id)` or `.thread(WebhookThread::Create(name))` in forums; declare stored webhooks' channel with `.in_channel(kind)` to fail before sending |
| `WebhookError::Rejected { status: 403, code: Some(50013) }` on `create` / `Rejected { status: 404, code: Some(10015) }` on a send | the bot lacks `MANAGE_WEBHOOKS`, or the stored webhook was deleted | ask for the permission; on 10015 create a new webhook and replace the stored id and token |
| `WebhookError::InvalidToken` | a stored token that is not 1–512 characters of `[A-Za-z0-9._-]` (cut, quoted, or the whole webhook URL) | store `hook.token()` as returned by `create` |
| `CacheError::Disabled` (`Discord cache resource USER is not enabled: add it with AppBuilder::discord_cache(DiscordCacheConfig::default().with_resources(..))`) | `ctx.cache()` read a resource the App does not cache (the default is `GUILD \| CHANNEL \| ROLE`), or the cache is `DiscordCacheConfig::none()` | add the resource with `.discord_cache(DiscordCacheConfig::default().with_resources(DiscordCacheResources::USER))` (`MEMBER` also needs `GUILD_MEMBERS`); `Ok(None)` instead means not cached yet: fall back |
| `WARN noctis::cache: Discord cache updates are failing; events still reach their handlers` | the `DiscordCache` backend errs or takes over 1 s (`noctis_discord_cache_update_errors_total`) | fix the backend; one warning per outage, an `INFO … recovered` follows |
| "This interaction failed" | handler panicked before the deadline on an `auto_defer = false` handler, or slow work before `r.show_modal(..)` | keep modal-opening handlers fast (< 3 s); load data after opening if possible; never set `auto_defer = false` on a handler that does not open a modal |
| "This interaction failed" / `AckDeadlineMissed` (`interaction not acknowledged in time: … past the 2.9 s ack deadline`) | the interaction waited in the queue past the deadline, so even the auto-defer failed (`WARN noctis::dispatch: auto-defer failed handler=..`), or an `auto_defer = false` handler was slow | Commands are auto-deferred at `ack_deadline` (2000 ms) and answer late by editing; keep `ack_deadline` ≤ 2000 ms, raise `.handler_concurrency(n)` if lanes back up. Check `noctis_ack_latency_seconds`, `noctis_http_phase_seconds{phase="ttfb"}` and `noctis_interaction_ack_hedges_total` for a slow network |
| `ERROR noctis::dispatch: an `auto_defer = false` handler did not answer within the 3 s ack deadline handler=.. module=..` | an `auto_defer = false` handler did slow work before `r.show_modal(..)` (no auto-defer for it) | open the modal first; load data after, or pre-load it cheaply |
| `ModalUnavailable` (`a modal can only be the first response to an interaction`) | `r.show_modal(..)` over the `Dispatcher` (prefix commands), or after another initial response | answer prefix invocations with a message; open modals only from interactions |
| `NoReply` (`there is no reply message to fetch`) | `acked.fetch_reply()` over the `Dispatcher` before the handler posted its reply (a `defer_reply` there is only a typing indicator) | reply first, or keep the message you sent |
| the first Discord request after a quiet period is slow, or `RequestTimedOut` after ~10 s | a NAT/firewall silently dropped an idle connection, or slow DNS | nothing to add: the HTTP edge keeps one connection warm with HTTP/2 PINGs and redials dead ones; do **not** poll `GET /users/@me`. If it persists, lower `HttpConfig::ping_interval` and check `noctis_http_ping_timeouts_total` and `noctis_http_phase_seconds{phase="dns"}` |
| `ERROR noctis::http: the Discord HTTP edge could not start` | the tokio runtime has no IO driver | build it with `enable_all()` (as `#[tokio::main]` does) |
| a click or modal submit runs nothing and the user sees the generic ephemeral "Something went wrong" notice; `WARN noctis::dispatch: component interaction rejected reason=.. shard=.. route=..` | `unknown_route`: no mounted Module adds the Route (or it is the other family); `kind_mismatch`: a component of another kind carries the id; `malformed`/`undecodable`: a hand-built or tampered `custom_id`; `forged`: a `signed` Route's tag does not verify (tampered, or signed with a key that is no longer `NOCTIS_PAYLOAD_KEY`/`NOCTIS_PAYLOAD_KEY_PREVIOUS`); `undecodable` after a deploy: the Payload changed without a `schema` bump (anything but appending an `Option` field) | build components only with `X { .. }.build()`; keep Route ids and Payload shapes stable (§ schema evolution in `routes-payloads-refs.md`); rotate keys with `NOCTIS_PAYLOAD_KEY_PREVIOUS` |
| the user sees the generic notice and the sink gets ``handler `{handler}` takes `Ext<{T}>`, but no Middleware inserted it in this run`` (`ExtensionMissing`) | the providing Middleware declared `.provides::<T>()` but returned `Flow::Continue` without `ctx.insert(..)` on some branch, or the handler was called through a detached test seam (no Middleware runs there) | insert `T` on every `Continue` branch, stop with `Flow::Stop(Some(key))` instead, or take `Option<Ext<T>>` |
| a handler never runs and the user sees nothing (or "This interaction failed") | a before-Middleware returned `Flow::Stop(None)` (`DEBUG noctis::middleware: stopped middleware=.. handler=..`) | deny with `Flow::Stop(Some(key))` so the user gets a notice |
| `WARN noctis::dispatch: could not send the error notice` | the notice of a failed handler or a rejected click could not be sent (Discord rejected it, the token expired, the follow-up cap) | nothing to fix in the handler; check the error field and Discord's status |
| `UpdateUnavailable` (`there is no message to update: this modal was not opened from a component message`) | `ctx.update(..)`, `r.update(..)` or `defer_update()` in the submit handler of a modal that a slash command opened (also after the auto-defer); nothing was sent | answer with `ctx.respond(msg)` (a new message); a modal opened from both a button and a command can try `ctx.update` and fall back to `ctx.respond` on this error (`responder.md` §1b) |
| `ResponderError::Invalid` / `UiError::Limits` with ``[CID-02] row[1].select[0].custom_id: limit a custom_id unique within the message, actual `route `x`: equal Payload` `` (or the resolved id), `= note: first used at …`, `= help: give the components a Payload field that tells them apart, …` | two components built at runtime with the same Route and Payload (a select, signed or by-Ref Route too); nothing was sent | add a Payload field that tells them apart (target page, row id), or render only one |
| a select keeps showing the user's previous pick after `ctx.update(..)` | the render does not set the current value as the default | set `.default(true)` on the current option, `default_users(..)` etc.; every send gets a fresh nonce, so the client shows the render's defaults |
| user text renders as bold, headings or links, or `@everyone`/`<@id>` in a name renders as a mention | user text was put in a Text Display unescaped | `escape_markdown(&text)`, plus `escape_mentions(..)` where mentions must not render (once each) |
| `AlreadyResponded` (`this handler already sent its initial response; nothing was sent`) | a second initial response in one handler: `ctx.update(..)` after `ctx.respond`/`ctx.update`/`ctx.reply_text` or a Responder's `reply`/`defer_update`/…, or a Responder Pending method (`reply`, `update`, `defer_reply`, `show_modal`, …) after a ctx method; or any ctx method after `show_modal` | answer once: use `ctx.respond`/`ctx.reply_text` (they follow up once answered) or the Acked Responder's `edit_reply`/`follow_up` for more messages; do not mix a ctx method and a Responder for the initial response |
| clicking an old message shows "This interaction has expired" (`DEBUG noctis::dispatch: component interaction expired reason=stale\|ref_unresolved hook=default`) | the Route's `schema`, body kind or flags changed since the message was sent (stale), or its Ref is missing (expired after its `ttl`/`.ref_ttl`, consumed by a `one_shot` click, deleted with `RefOps`, or lost with a memory `RefStore` on restart) | add `#[on_stale]`/`#[on_expired]` methods that re-render the View from scratch, or one App-wide `.expired_handler(f)`; `routes-payloads-refs.md` §6 and §8 |
| clicks on another user's panel say "Only the person who started this can use it." (`DEBUG noctis::guard: denied … guard="owner"`) | the `owner = …` Route Guard working as designed | make the View ephemeral or remove the Guard |
| a guarded button says "You don't have permission to use this." in DMs, or to a member who seems to have the role (`DEBUG noctis::guard: denied … guard="permissions"`) | a `permissions = …` Guard denies every DM click (there is no member), and it reads the member's resolved permissions in that channel, so a channel overwrite can remove one | keep guarded components in guilds (`contexts = [Guild]` on the command that sends them), or check the channel's overwrites; `ADMINISTRATOR` always passes |
| a `one_shot` confirmation briefly shows "This interaction has expired" to its owner right after someone else clicked it, or `WARN noctis::refs: could not restore the one-shot Ref after a Guard denial` | a Guard denied the other click and was putting the record back (a short window), or the `RefStore` failed or refused the restore | click again; if the warning repeats, check the `RefStore` |
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
| `[INT-07] choices: limit at most 25, actual 40` (`= help: Noctis sends the first 25 choices and logs this as a warning`) | the autocomplete returned too many suggestions | cap the query at 25 (`LIMIT 25`) or `.take(25)` before `collect()` |
| warn `noctis::autocomplete` `[INT-07] too many autocomplete choices: sent the first ones` (`returned=40 sent=25`) | an `#[autocomplete]` method returned more than 25 suggestions | cap the query at 25, or `.take(25)` before `collect()` |
| warn `noctis::autocomplete` `invalid autocomplete choices dropped` (`dropped=n rules=OPT-09`) | an empty suggestion label (a longer one than 100 is cut with `…` and a warning), a STRING value over 100, a value of another type than the option, or a number out of range | fix the suggestions; the value type is the field's type (`String`, `i64`, `f64`) |
| `` `autocomplete` is a bare flag: write `#[option(.., autocomplete)]` and add `#[autocomplete(field)] async fn …` `` | the withdrawn `autocomplete = path` form | the bare flag plus the `#[autocomplete(field)]` method (`commands.md` §4) |
| Autocomplete Error Sink: `the autocomplete handler did not respond within 2.5s` (`AutocompleteError::DeadlineMissed`) | the method took longer than its budget (2.5 s from receipt); Discord got an empty list | cache the data (a port), bound slow lookups with `tokio::time::timeout`, answer from what you have |
| warn `noctis::dispatch` `no autocomplete handler` | Discord sent autocomplete for an option without an autocomplete method (stale published commands) | publish again, or add the `#[autocomplete(field)]` method |
| `Budget::fits` returns 0 | not even one row fits beside the fixed parts | shorten the row, render it as an image, or show it on a detail screen |
| `RenderError::Overloaded` | render queue full | show the busy notice; raise pool/queue in `ImageConfig`; cache more |
| `RenderError::CapExceeded` | too many pixels/frames/fps | smaller size, fewer frames, lower fps |
| card text is invisible / boxes for emoji | no font / no emoji font | `default-font`; enable the `image-emoji` feature (bundled Twemoji) |
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
| Users see "Something went wrong. Please try again later." | a Command handler returned `Err` (without `with_message`) or panicked | find the `ERROR noctis::command: command handler failed` / `command handler panicked` line (fields `handler`, `error`/`panic`) or your `Command` Error Sink; fix the cause, or return `HandlerError::user(key)` / `.with_message(key)` for a known condition (`responder.md` §5) |
| `WARN noctis::i18n: missing message key key="shop.save_failed"` | a notice used a key no registered catalog defines, so the user saw the generic text | add the key to `locales/en-US.yaml` and register the catalog with `AppBuilder::locales(strings::catalog())` (`i18n.md` §2) |
| `WARN noctis::dispatch: could not send the error notice handler=..` | the notice hit the follow-up cap (`FollowupCap`), an expired token (`TokenExpired`) or a Discord rejection | nothing to retry; the error itself is still in the log and the Error Sink |
| `ERROR noctis::error_sink: error sink panicked` | your `ErrorSink::report` panicked (caught; workers keep running) | fix the sink; never `unwrap` inside it |
| commands don't appear | not published | `cargo run --release -- --publish-guild <id>`; global publish can take time |
| `--publish` fails with ``[META-04] command `me` declares integration type `user_install`, which this application has not enabled`` | a global Command's `integration_types` names an install type not enabled for the application (nothing was written; a dry run reports it too) | enable it under Installation in the Developer Portal, or remove it from `integration_types` |
| `--publish-guild` fails with ``[REG-03] two chat_input commands are named `ping` in guild 42`` (or `[REG-02] guild 42 would hold 101 chat_input commands`) | the dev mirror puts every global Command next to the guild's own `guilds = [..]` Commands; a name is used in both, or the union is too large | rename one, or test the guild Command alone; nothing was written |
| `Error::Cli(CliError::UnknownFlag { flag: "--test-threads=…" })` (or another harness flag) when a test starts an App | `app.run()` parses the process arguments, and `cargo test` passes its own | start test Apps with `app.run_with_args(std::iter::empty::<&str>())`; for your own CLI flags use `NoctisArgs::split` |
| ``environment variable `NOCTIS_FEATURES` is not a Boot Flag selection: {reason}`` (`EnvError::InvalidBootFlags`) | `NOCTIS_FEATURES` is not `all`, `none` or `a,b` (spaces, empty items, uppercase, repeats) | write it like `economy,beta` |
| ``` `--features` got `{value}`: {reason}``` / ``` `--features` needs a value: … ``` (`CliError::InvalidFeatures` / `MissingFeatures`) | a malformed `--features` (from `App::from_env()` or `run`) | pass `--features a,b`, `--features all` or `--features none` |
| ``` `--features {given}` does not match the Boot Flags this App was built with (`{built}`): … ``` (`CliError::BootFlagsMismatch`) | the App was built with another selection (`App::builder()` without `.boot_flags`, or a different `.boot_flags(..)`) and `run` got `--features` | build with `App::from_env()`, which reads `--features`, or pass the same selection to `.boot_flags(..)` |
| a Command published before is deleted by `--publish --features a` | a Publish of a subset overwrites the remote list with it | publish from a run with the full selection (`all`) |
| `--publish-guild` fails with `CliError::MissingDevGuild` | no id and no dev guild | pass the id, call `.dev_guild(id)` or set `NOCTIS_DEV_GUILD` |
| `Publish` rewrites commands on every run | a remote field Noctis does not send differs (for example localizations set by hand in another tool) | expected once; the next run reports `commands unchanged; nothing written` |
| shard closes with 4014 | privileged intent not enabled in the portal | enable it, or drop the handler that needs it |
| `run()` returns `StartError::DisallowedIntents { shard }` | close 4014 during startup: a privileged intent is not enabled in the portal | enable it, or stop requesting it |
| `run()` returns `StartError::InvalidToken` | `GET /gateway/bot` answered 401, or close 4004 | copy the token again from the Developer Portal |
| `run()` returns `StartError::ShardTotalTooSmall { total, minimum }` | a fixed shard total below ⌈recommended × 2 / 5⌉ (GW-03) | remove `shard_total` or raise it to at least `minimum` |
| `run()` returns `StartError::SessionStartLimit { .. }` | the daily identify budget can't cover every shard (GW-02) | wait `reset_after`, or run fewer shards |
| warning "shard total is not a multiple of max_concurrency" | a fixed total misaligned with Discord's identify buckets | use a multiple of `max_concurrency`, or ignore it (startup is only slower) |
| startup warning "Cluster without HTTP proxy" | several processes, no `NOCTIS_HTTP_PROXY` | configure a shared proxy |
| two processes disagree on module state | different `ModuleStateStore`s, or different `app_id`s over one store | share one Adapter under one `app_id`; propagation ≤ `.module_state_poll(..)` (default 5 s) |
| ``unknown Module `Profile`: pass the id of a mounted Module (as written in `Module::new(..)`) that the Boot Flags select`` (`ModuleStateError::UnknownModule`) | `ctx.modules().set_enabled`/`reset` with an id in another case, a typo, or a Module the Boot Flags pruned (`is_enabled` answers `false` for it) | use the exact `Module::new(..)` id; list the valid ones with `ctx.modules().ids()` |
| `the ModuleStateStore failed: …` (`ModuleStateError::Port`) | the `ModuleStateStore` Adapter errored (or a detached test context has no App) | check the store; the snapshot keeps its last state |
| `could not read the Module state from the ModuleStateStore (…): check the store and retry; nothing was sent` (`PublishError::ModuleState`) | Publish reloads the Module state before any request and the store failed | fix the store and re-run `--publish`; `--publish-clear` does not need it |
| a command, button or modal answers "This feature is disabled here." | its Module (or an ancestor) is disabled in this guild or globally (`ctx.modules()`) | `ctx.modules().set_enabled(id, scope, true)` or `reset(id, scope)`; re-run `--publish` to update guild-scoped Commands |
| `WARN noctis::modules: could not read the Module state; keeping the last snapshot` | the `ModuleStateStore`'s `version()` or a record read failed during a poll | check the store; one `info` logs the recovery |
| Ref minted on one process "expired" on another | memory `RefStore` in a Cluster | shared `RefStore` Adapter |

## Where to look

1. The error's `help:` line (always actionable).
2. `limits-cheatsheet.md` for the rule id.
3. The topic reference (`commands.md`, `components-v2.md`, …).
4. If still unclear: report that the Noctis skill doesn't cover it. Do not read the
   framework source.

## Rule id index

Every rule id of the Discord Rules Catalog, in catalog order. Find the id an error names,
read what the rule means and apply the fix in the reference it names. Numeric limits are in
`limits-cheatsheet.md`; the full rule text is in the
[catalog](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md).

| Rule | Meaning | Fix / where |
| --- | --- | --- |
| CMD-01 | command name word: letters, digits, `-`, `_`, `ʼ` only, 1-32 | rename the word in `#[command("..")]` or `cmd.<path>.name`; `commands.md` §1 |
| CMD-02 | 1-32 characters per command name word | shorten the word; `commands.md` §1 |
| CMD-03 | command names are lowercase | lowercase the pattern word or the localized `cmd.<path>.name`; `commands.md` §1, `i18n.md` §4 |
| CMD-04 | description 1-100 characters, read from `cmd.<path>.description` | add or shorten the key in `locales/en-US.yaml`; never a `description = "…"` literal; `commands.md` §1, `i18n.md` §4 |
| CMD-05 | ≤ 8000 characters per command (options, choices, longest localizations) | shorten descriptions or choices, or split the command; `commands.md` §6 |
| CMD-06 | option names unique per level | rename the field or `cmd.<path>.opt.<name>.name`; `commands.md` §3 |
| CMD-07 | `nsfw` consistent under one base | set `nsfw = true` on every leaf or on the base (`Module::base`); `commands.md` §6 |
| CMD-10 | at most command → group → subcommand (3 words) | shorten the Command Pattern; `commands.md` §2 |
| CMD-11 | a name is either a subcommand or a group | rename one of the paths (`config set` vs `config set prefix`); `commands.md` §2 |
| CMD-12 | a base with subcommands takes no options and is not invokable | move the base's options to a subcommand; `commands.md` §2 |
| CMD-13 | ≤ 25 options / subcommands / groups per level | split the command or use a select View; `commands.md` §3 |
| CMD-14 | groups and subcommands may coexist | nothing to fix (documentation only); `commands.md` §2 |
| CMD-15 | ≤ 25 expansions per Command Pattern | fewer alternatives, or `max_expansions = n`; `commands.md` §2 |
| CMD-16 | Command Pattern syntax: whole-word groups, `?` only after a group | rewrite the pattern as `a (b\|c) d?`-style words; `commands.md` §2 |
| OPT-01 | required options before optional ones | put `Option<T>` fields after the required ones; `commands.md` §3 |
| OPT-02 | option types come from the field type | use a supported field type (`String`, `i64`, `f64`, `bool`, `ResolvedUser`, …); `commands.md` §3 |
| OPT-03 | INTEGER options within ±(2^53 − 1) | tighten `min_value`/`max_value`; `commands.md` §3 |
| OPT-04 | NUMBER options within ±2^53 | tighten `min_value`/`max_value`; `commands.md` §3 |
| OPT-05 | `min_value`/`max_value` only on numbers, `min ≤ max` | fix the bounds in `#[option(..)]`; `commands.md` §3 |
| OPT-06 | `min_length` 0-6000, `max_length` 1-6000, strings only | fix the bounds in `#[option(..)]`; `commands.md` §3 |
| OPT-07 | `channel_types` only on channel options | remove it or change the field type; `commands.md` §3 |
| OPT-08 | ≤ 25 choices per option | fewer `Choices` variants, or autocomplete; `commands.md` §3 |
| OPT-09 | choice name 1-100, string value ≤ 100 | shorten the label or value (`cmd.<path>.opt.<name>.choice.<value>`); `commands.md` §3 |
| OPT-10 | choices and autocomplete are exclusive | drop one of them; `commands.md` §4 |
| OPT-11 | autocomplete only on string / integer / number options | change the field type or drop `autocomplete`; `commands.md` §4 |
| OPT-12 | autocomplete input is unvalidated (`Partial<Self>`) | re-validate the value in `handle`; `commands.md` §4 |
| OPT-13 | `file_types` ≤ 10, attachment options only | fix the `file_types` list; `commands.md` §3 |
| OPT-14 | `required` never on subcommands or groups | nothing to fix (by construction); `commands.md` §3 |
| LOC-01 | locale file names are Discord locale codes | rename `locales/<code>.yaml` (`en-US`, `tr`, …); `i18n.md` §1 |
| LOC-02 | localized names and descriptions follow CMD/OPT/CTX rules | fix the reported `cmd.*`/`cmdword.*`/`ctxcmd.*` value; `i18n.md` §4 |
| LOC-03 | localized option names unique per command per locale | rename the localized option name; `i18n.md` §4 |
| LOC-04 | partial localization falls back to `en-US` (build warning) | add the key to the locale file; `i18n.md` §2 |
| LOC-05 | alias of CMD-05: the longest localization counts toward 8000 | shorten the longest translation; `i18n.md` §4 |
| LOC-06 | dispatch uses the default (English) names | nothing to fix; match on English names only; `i18n.md` §4 |
| CTX-01 | context-menu name 1-32 characters | shorten the name or `ctxcmd.<key>.name`; `commands.md` §5 |
| CTX-02 | context menus take no description and no options | remove them (a unit struct); `commands.md` §5 |
| CTX-03 | command types (chat input, user, message, entry point) | nothing to fix (picked by the derive); `commands.md` §5 |
| META-01 | `permissions = ..` default member permissions | use twilight `Permissions` names (`MANAGE_GUILD \| BAN_MEMBERS`); `commands.md` §1 |
| META-02 | metadata belongs to the base command | make the leaves agree or declare it once with `Module::base(..)`; `commands.md` §6 |
| META-03 | `contexts`/`integration_types` only on global commands | drop them from a `guilds = [..]` command; `commands.md` §1 |
| META-04 | `integration_types` must be enabled for the app | enable the install type in the Developer Portal, then Publish again; `commands.md` §7 |
| META-05 | `dm_permission` is deprecated | use `contexts = [..]`; `commands.md` §1 |
| REG-01 | 100 chat-input, 15 user, 15 message global commands | merge commands with Command Patterns; `commands.md` §6-7 |
| REG-02 | the same counts per guild | fewer guild commands, or disable Modules per guild; `commands.md` §6-7 |
| REG-03 | unique names per type per scope | rename one command; `commands.md` §6 |
| REG-04 | merged Module trees must not conflict | fix the conflict the `BuildError` notes name; `commands.md` §6 |
| REG-05 | 200 command creates per day per guild | publish less often (Publish is diff-based); `commands.md` §7 |
| REG-06 | ≤ 100 permission overwrites per command (Bearer token) | nothing to fix in a bot (documentation only); `commands.md` §6 |
| INT-01 | answer within 3 s | keep the auto-defer, defer before slow work; `responder.md` §2 |
| INT-02 | interaction token valid 15 min (`TokenExpired`) | edit or follow up sooner, or send a new message; `responder.md` §4 |
| INT-03 | exactly one initial response | answer once (`ctx.respond`/`ctx.update`); `responder.md` §4 |
| INT-04 | response types per interaction kind (no `update` on a Command) | `ctx.respond` in a Command, `ctx.update` in a component/modal handler; `responder.md` §1b |
| INT-05 | a modal is the first response; no modal after a modal | `auto_defer = false` + the `Manual` responder + `show_modal`; `responder.md` §2b, `modals.md` §3 |
| INT-06 | autocomplete cannot be deferred (2.5 s) | return `Suggestions<T>` fast; `commands.md` §4 |
| INT-07 | ≤ 25 autocomplete choices (truncated) | cap the query at 25; `commands.md` §4 |
| INT-08 | a deferred reply carries only `EPHEMERAL` | nothing to fix (by construction); `responder.md` §2 |
| INT-09 | allowed callback message flags | nothing to fix (by construction); `responder.md` §1 |
| INT-10 | ephemerality is fixed at creation | declare `response = ephemeral` on the attribute; `responder.md` §3 |
| INT-11 | ≤ 5 follow-ups for a user-installed app (`FollowupCap`) | send fewer follow-ups, edit instead; `responder.md` §4 |
| INT-12 | the first follow-up after a defer edits the original | nothing to fix (Noctis edits explicitly); `responder.md` §4 |
| INT-13 | interaction callbacks skip the global HTTP limit | nothing to fix; `app-and-sharding.md` §5 |
| INT-14 | `rate_limit(scope, n, per = "..")` arguments | fix the attribute (scope, count 1..=4294967295, window 1 s..=1 d); `modules-middleware-ratelimits.md` §7 |
| CID-01 | `custom_id` ≤ 100 characters (proven at compile time) | `by_ref`/`overflow = spill`, smaller fields, or a shorter Route id; `routes-payloads-refs.md` §5 |
| CID-02 | `custom_id`s unique per message | give the Route a Payload field that differs; `routes-payloads-refs.md` §1 |
| CID-03 | Route id `[a-z0-9._-]{1,24}`, not starting with `_` | rename the Route id; `routes-payloads-refs.md` §1 |
| CID-04 | one handler per Route id per App | `.add::<X>()` in exactly one mounted Module, `uses::<X>()` elsewhere; `routes-payloads-refs.md` §1 |
| CID-05 | Refs are random and bound to the Route | nothing to fix (by construction); `routes-payloads-refs.md` §7 |
| CID-06 | `custom_id` is untrusted client input | authorize in Route Guards and the handler; `routes-payloads-refs.md` §8 |
| CID-07 | encoded Routes are ASCII only | nothing to fix (by construction); `routes-payloads-refs.md` §4 |
| CV2-01 | the Components V2 flag on every V2 send | nothing to fix (by construction); `components-v2.md` §12 |
| CV2-02 | no `content`/`embeds` with Components V2 | build a View (UD-1); `components-v2.md` §12 |
| CV2-03 | ≤ 40 components per message, every depth | size pages with `noctis::ui::Budget`; `components-v2.md` §16, `views-and-recipes.md` §4 |
| CV2-04 | ≤ 4000 Text Display characters per message | shorten text or paginate; `components-v2.md` §2 |
| CV2-05 | component placement (what may hold what) | move the component per the placement matrix; `components-v2.md` §13 |
| CV2-06 | an Action Row: 1-5 buttons or exactly 1 select | split into rows; `components-v2.md` §8 |
| CV2-07 | a Section: 1-3 Text Displays + one accessory | fix the Section's children; `components-v2.md` §3 |
| CV2-08 | button rules (label ≤ 80, Link url ≤ 512, …) | fix the button fields; `components-v2.md` §9 |
| CV2-09 | select rules (placeholder ≤ 150, ≤ 25 options, values) | fix the select fields or page the options; `components-v2.md` §10 |
| CV2-10 | a Media Gallery holds 1-10 items | fewer items; `components-v2.md` §5 |
| CV2-11 | Thumbnail description ≤ 1024, images only | fix the Thumbnail; `components-v2.md` §4 |
| CV2-12 | a File component references `attachment://` only | upload the file with the message; `components-v2.md` §6 |
| CV2-13 | Separator spacing 1-2, accent colour 24-bit | fix the value (`rgb!`); `components-v2.md` §7 |
| CV2-14 | numeric component ids unique per message | nothing to fix (by construction); `components-v2.md` §12 |
| CV2-15 | legacy messages: ≤ 5 Action Rows | use a V2 View; `components-v2.md` §12 |
| CV2-16 | unknown component types round-trip as raw JSON | use `RawComponent`; `components-v2.md` §11 |
| CV2-17 | media URL ≤ 2048 characters | shorten the URL or upload the file; `components-v2.md` §4 |
| MOD-01 | modal title ≤ 45 characters | shorten `defaults(title = ..)`; `modals.md` §1 |
| MOD-02 | 1-5 top-level modal components | fewer inputs, or a second step; `modals.md` §1 |
| MOD-03 | modals hold no buttons, Sections or `disabled` | only Labels with inputs and Text Displays; `modals.md` §4 |
| MOD-04 | Label `label` ≤ 45, `description` ≤ 100 | shorten the Label texts; `modals.md` §4 |
| MOD-05 | Text Input lengths ≤ 4000, placeholder ≤ 100 | fix the bounds or the pre-fill; `modals.md` §4 |
| MOD-06 | File Upload `min_values` 0-10, `max_values` 1-10 | fix the counts; `modals.md` §4 |
| MOD-07 | Radio 2-10 / Checkbox Group 1-10 options | fix the options; `modals.md` §4 |
| MOD-08 | alias of CID-01 for the modal `custom_id` | see CID-01; `modals.md` §1 |
| MOD-09 | submitted values and uploads are untrusted | validate in the handler, download promptly; `modals.md` §3.1 |
| MSG-01 | legacy `content` ≤ 2000 characters | shorter text, or a V2 View; `responder.md` §6 |
| MSG-02 | embed limits | nothing to fix (documentation only; embeds are not used, UD-1); `responder.md` §6 |
| MSG-03 | allowed-mentions `parse` excludes explicit ids | nothing to fix (by construction); `views-and-recipes.md` §5 |
| MSG-04 | ≤ 100 user and 100 role mention ids | fewer ids in `Mentions`; `views-and-recipes.md` §5 |
| MSG-05 | mention defaults: Text Display mentions ping | set `Mentions` explicitly; `views-and-recipes.md` §5 |
| MSG-06 | `nonce` ≤ 25, ≤ 3 stickers | shorten the nonce / fewer stickers; `responder.md` §6 |
| MSG-07 | message flags per endpoint | nothing to fix (by construction); `responder.md` §1 |
| MSG-08 | an edit replaces components and attachments | re-send the whole View, keep attachments by id; `components-v2.md` §12 |
| MSG-09 | a message needs content (a V2 one ≥ 1 component) | add a component; `components-v2.md` §12 |
| ATT-01 | file size within `attachment_bytes` (default 20 MiB) | shrink the file; `components-v2.md` §12 |
| ATT-02 | ≤ 25 MiB per request | fewer or smaller files; `components-v2.md` §12 |
| ATT-03 | ≤ 10 attachments per message | fewer files; `components-v2.md` §12 |
| ATT-04 | `files[n]` naming | nothing to fix (Noctis names parts); `components-v2.md` §12 |
| ATT-05 | `attachment://` must name a file in the request | attach the file or keep it by id; `components-v2.md` §12 |
| ATT-06 | alt text ≤ 1024; rendered images need alt text | add `alt!(..)`; `components-v2.md` §12, `images.md` |
| ATT-07 | `attachment://` image formats | use png, jpg, webp or gif; `components-v2.md` §12 |
| WH-01 | webhook components need `with_components` | nothing to fix (by construction); `responder.md` §7 |
| WH-02 | a webhook you don't own sends only non-interactive components | send a `StaticMessage`; `responder.md` §7 |
| WH-03 | V2 webhook bodies: no content or embeds | nothing to fix (by construction); `responder.md` §7 |
| WH-04 | webhook name 1-80, no `clyde`/`discord` | rename (`webhook_name!`); `responder.md` §7 |
| WH-05 | forum and media channels need a thread | `.thread(..)` on the webhook send; `responder.md` §7 |
| RL-01 | per-route HTTP buckets | wait the `retry_after` of `RateLimited`; `app-and-sharding.md` §5 |
| RL-02 | 50 requests/s per bot token, shared | point every process at one proxy (`NOCTIS_HTTP_PROXY`); `app-and-sharding.md` §4 |
| RL-03 | 10,000 invalid requests per 10 min per IP | never retry 401/403/429 in a loop; `app-and-sharding.md` §5 |
| RL-04 | undocumented per-resource limits | throttle channel edits; `app-and-sharding.md` §5 |
| GW-01 | identify concurrency buckets | share one `IdentifyQueue` across a Cluster; `app-and-sharding.md` §4, `adapters.md` §7 |
| GW-02 | 1000 identifies per day | keep sessions resumable (`SessionStore`); `app-and-sharding.md` §4 |
| GW-03 | ≤ 2500 guilds per shard | more shards (`NOCTIS_SHARD_TOTAL`); `app-and-sharding.md` §4 |
| GW-04 | 120 gateway sends per 60 s | fewer gateway commands; handle `GatewayError`; `app-and-sharding.md` §6 |
| GW-05 | privileged intents must be requested and enabled | `.intents(..)` and the Developer Portal toggle; `app-and-sharding.md` §1, `modules-middleware-ratelimits.md` §4 |
| GW-06 | `large_threshold` 50-250 | `LargeThreshold::new::<N>()` in range; `app-and-sharding.md` §6 |
| GW-07 | Request Guild Members limits | one guild, ≤ 100 user ids, wait on `RATE_LIMITED`; `app-and-sharding.md` §6 |
| TW-01 | Template classes must be known Tailwind v4 classes | fix the class or add the token to `noctis-theme.css`; `images.md` §4 |
| TW-02 | `noctis-theme.css` grammar | only `@theme { --x: v; }` and `@keyframes`; `images.md` §5 |
| IMG-01 | text needs a registered font | enable `default-font` or `ImageConfig::font`; `images.md` §6 |
| IMG-02 | static image id and format | rename the id or convert to PNG/JPEG/WebP, register it; `images.md` §6 |
| IMG-03 | remote images only from declared `#[asset]` fields | declare the field, allow the origin; `images.md` §7 |
| IMG-04 | Discord emoji tokens only in data fields | move the `<:name:id>` token into a data field; `images.md` §6 |
| VC-01 | voice op 4 goes through the owning shard and GW-04 | use `ctx.voice()`; handle `VoiceError`; `voice.md` |
| VC-02 | one voice connection per guild | one call per guild; a newer join replaces it; `voice.md` |
