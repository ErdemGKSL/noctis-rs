# Research: tescord (primary) and dbi (lineage), a feature catalogue for noctis

- **Date:** 2026-10-19
- **Purpose:** Input for designing **noctis**, a Rust Discord bot framework on twilight-rs that reproduces the developer experience of `tescord`.
- **Primary reference:** `tescord@0.0.6` (npm), plus its companion `@tescord/type-gen@0.0.5`, plus how `/home/user/schwi-v2/apps/discord-bot` actually uses them.
- **Secondary reference (lineage only):** `@mostfeatured/dbi@0.2.37` (npm, by the same author family, TheArmagan / "MostFeatured"). We used dbi only to fill gaps and to explain where tescord's design came from.
- **Method:** We downloaded both npm tarballs and read them as data; nothing was executed. We read the compiled `dist/**/*.js` and `.d.ts`, the README, `.github/copilot-instructions.md` and `todo.md`. We read the schwi-v2 sources in full for the core files and sampled the modules.

---

## 1. Summary

tescord is the successor to dbi (and to "Tessen", per its README tagline). It is an Express-like, **object-parameter**, **composable** framework over discord.js v14. Its central ideas:

1. **Packs** are nestable containers. `pack.use(subPack | inspector | locale)` composes them, and **every registration returns a dispose callback**, which makes hot load and unload a first-class feature. `Tescord` itself *is* a Pack, the root pack, and also owns the clients.
2. **Pattern-named slash commands.** `name: "config (set|get) (prefix|welcome)?"` is expanded into every combination. Each combination is a one-, two- or three-word path that is split into *command / subcommand group / subcommand* at publish time. Registrations from *different packs* that share a base word, such as `partnership setup` and `partnership ban`, are **merged** into one Discord command.
3. **Typed component ids.** Buttons, selects and modals are registered with a stable `id` and default `options`. At render time you call `tescord.buildComponent({ id, data, overrides })`. The custom_id is `id + "䲜" + data joined with "䲜"`, and numbers are tagged with `"㥻"`. A code generator (`@tescord/type-gen`) turns registered ids into a `ComponentMap` type, so ids and overrides are type-checked.
4. **Two-layer i18n.** *Content* locales are YAML/JSON trees whose string leaves become callable formatters: `locale.user.punishHistory.header(a, b, c)`. *Interaction* locales translate command names word by word (`names: { punish: ceza }`) and translate descriptions, options and choices into Discord `*_localizations` at publish time. Every handler gets `locale.user` (from the interaction locale) and `locale.guild` (from the guild's preferred locale), both falling back to `defaults.language`. Type-gen also emits a typed `Localization` interface.
5. **Middleware via a result-collecting event bus.** `beforeEvent` and `beforeInteraction` listeners can return `false` to cancel, or return an object to merge into the context. Pack events propagate down to sub-packs. There are lifecycle events (`pack:loaded`, `interaction:registered`, `tescord:clientReady` and others) and error sinks (`tescord:interactionHandlerError`, `tescord:eventHandlerError`).
6. **Multi-client.** `clients: [{ id, token, options }]`. Each command can target a `clientId`, and publish groups commands per client. Sharding is not built in; schwi-v2 uses discord.js `ShardingManager` (process mode) and spawns extra processes for per-guild "custom bots".

Compared with dbi, tescord **dropped** several dbi features that noctis should bring back as opt-in modules:
- the reference store (objects in custom ids with TTL)
- inline (anonymous, TTL'd) listeners
- built-in rate limits
- `flag` filters on load
- prefix "message commands" that emulate slash commands
- `validate` hooks on options
- ordered/awaited event execution
- Svelte/Eta "HTML Components V2"

tescord keeps a *hook* for custom-data codecs (`tescord:customData:encode/decode`) but ships no storage. **This is exactly where noctis's pluggable storage-adapter trait belongs.**

---

## 2. Feature catalogue

Legend: **T** = tescord, **D** = dbi only, **S** = how schwi-v2 uses it. "Noctis priority": P0 = must have for parity with schwi-v2, P1 = important, P2 = nice to have.

| # | Feature | Source | schwi-v2 uses? | Noctis priority |
|---|---------|--------|----------------|-----------------|
| 1 | Root framework object + config (`id`, `clients[]`, `defaults.language`) | T | yes | P0 |
| 2 | Packs: nestable modules, `use()`, dispose callbacks, `destroy()` | T | yes (pack tree) | P0 |
| 3 | Hot load/unload (dispose handles, `refresh()` cache rebuild) | T (+D `load/unload`) | indirectly | P1 |
| 4 | Slash commands with pattern names → command/group/subcommand | T (D: plain space names) | yes, heavily | P0 |
| 5 | Cross-pack merge of commands sharing a base name | T | yes (`partnership setup`/`ban`) | P0 |
| 6 | Option declaration as a keyed map (types, min/max, length, channelTypes) | T (D: builder fns) | yes | P0 |
| 7 | Choices as `{ value: displayName }` maps | T | yes | P0 |
| 8 | Autocomplete inline on the option (`autoComplete(ctx) → map`) | T (D: `onComplete`) | yes (DB lookups) | P0 |
| 9 | `defaultMemberPermissions`, `contexts`, `nsfw` | T (D: `directMessages`) | yes | P0 |
| 10 | User/Message context menus | T, D | no | P1 |
| 11 | Component registry: button, 5 select types, modal with default options | T, D | yes, ~130 ids | P0 |
| 12 | `buildComponent({id, data, overrides})` → deep-merged JSON | T (D: `toJSON({overrides, reference})`) | yes, everywhere | P0 |
| 13 | Custom-id data encoding (strings/numbers) | T, D (richer) | yes | P0 |
| 14 | Custom-data codec hook (encode/decode events) | T | no (declared only) | P0 (as trait) |
| 15 | Reference store: objects by ref id, TTL auto-clear | D | — | P1 (adapter trait) |
| 16 | Inline (anonymous) components/events with TTL | D | — | P2 |
| 17 | Generated types: `ComponentMap`, `Localization` | T (+type-gen) | yes (`pnpm types`) | P0 (proc-macro/build.rs) |
| 18 | Content i18n: YAML/JSON, callable leaves, `{0}` placeholders, default-language fallback | T, D | yes | P0 |
| 19 | Interaction i18n: word-by-word names, descriptions, options, choices | T, D | yes (tr) | P0 |
| 20 | Per-handler `locale.user` / `locale.guild` | T, D | yes | P0 |
| 21 | Translation scaffold generator (example interactions.yaml) | S | yes | P2 |
| 22 | Gateway event handlers with named-arg context | T, D | yes (~20 events) | P0 |
| 23 | Middleware: `beforeEvent` / `beforeInteraction` (cancel or extend ctx) | T, D (+`after*`) | yes (custom-bot gating) | P0 |
| 24 | Error sinks: `interactionHandlerError`, `eventHandlerError`, `autocompleteError` | T, D | yes | P0 |
| 25 | Lifecycle events (`pack:*`, `interaction:*`, `tescord:client*`, `published`) | T | partly | P1 |
| 26 | Inspectors: pattern/wildcard secondary routers + manual `emit` | T | no | P2 |
| 27 | Multi-client with per-command `clientId` | T, D (`publish: ns`) | 1 client | P1 |
| 28 | Publish: global or guild, bulk PUT per client | T, D | yes (`--publish`, guild flag) | P0 |
| 29 | Sharding | D (hybrid) / S (ShardingManager) | yes | P1 |
| 30 | Rate limits (User/Channel/Guild/Member/Message) via store | D | hand-rolled (Redis, in-memory) | P1 |
| 31 | Feature flags on registrations (`flag`, `load("all")`) | D | hand-rolled (`features.has`) | P1 |
| 32 | Prefix message commands that emulate slash commands | D | hand-rolled (`s!` custom cmds) | P2 |
| 33 | Option `validate` hooks (autocomplete and result) | D | — | P2 |
| 34 | Ordered/awaited events, `triggerType` client selection | D | — | P2 |
| 35 | Components V2 (Container/TextDisplay/Section/...) | S (raw JSON), D (Svelte/Eta) | yes, exclusively | P0 (typed builders) |
| 36 | Generic KV `store` (`get/set/delete/has`) | D | Redis caches | P0 (trait) |
| 37 | Image rendering (satori→resvg→webp) | S (packages/satori) | not yet wired | P2 (separate crate) |

---

## 3. Per-feature sections

### 3.1 Root object and configuration (T)

```ts
const tescord = new Tescord({
  id: "misket",
  clients: [{ id: "misket-main", token: process.env.DISCORD_TOKEN!,
              options: { intents: ["Guilds", "GuildMessages", ...], partials: [...] } }],
  defaults: { language: "tr" }
});
tescord.use(i18n); tescord.use(integrations); tescord.use(modules);
tescord.start();          // or: tescord.publish(guildId?)
```

How it works internally:
- `Tescord extends Pack`. `start()` calls `refreshClients()`, which creates the `Client`s, then `refresh()`, which flattens the pack tree into `cache.{interactions, events, inspectors, locales, subPacks}` and merges locales.
- It then monkey-patches each `client.emit` so that every known discord.js event goes through `handleEvent`. It logs in sequentially and emits `tescord:clientReady` / `tescord:clientsReady`.
- The `Pack` id `"tescord"` is reserved.

In dbi the equivalent is `createDBI("ns", { discord, defaults, sharding, store, references, inlineListeners, strict, messageCommands })`, followed by `dbi.register(cb)`, `await dbi.load(...flags)` and `dbi.login()`. `strict: false` wraps handlers in try/catch and routes errors to `interactionError`/`eventError`. `strict: true` lets them throw.

### 3.2 Packs, composition, hot unload (T)

```ts
export const pack = new Pack({ id: "modules" });
pack.use(welcome); pack.use(tempVoice);
if (features.has("seacord")) pack.use(seacord);   // schwi-v2's manual feature flag
const dispose = pack.slashCommand({...});          // every registration returns () => void
pack.unload(dispose);  pack.destroy();             // destroy runs unloaders, clears data, emits pack:destroyed
```

How it works internally:
- Each Pack holds `data.{interactions, events, inspectors, locales, subPacks}` collections, all keyed by id.
- A duplicate interaction id within a pack throws. Ids are global in practice, because `cache.interactions` is keyed by id after flattening.
- `use()` returns a combined dispose.
- After a dispose you must call `tescord.refresh()` to rebuild the flattened cache. Nothing is automatic, so hot reload in tescord means: dispose, re-register, `refresh()`.

dbi's equivalent is `dbi.register(cb)` with an `onUnload(cb)` inside it. `dbi.unload()` removes everything registered and unhooks listeners. `dbi.load()` re-runs all register callbacks, which lets you re-`require` changed files (`Utils.recursiveImport` / `unloadModule` / `recursiveUnload`) for hot reload.

### 3.3 Slash commands with pattern names (T), and the space-split model from dbi

```ts
pack.slashCommand({
  id: "send-ticket-create-message",          // framework id, also the i18n key
  name: "send ticket-create message",        // 3 words → command "send", group "ticket-create", sub "message"
  description: "Send the ticket create message to the channel",
  defaultMemberPermissions: ["Administrator"],
  contexts: ["Guild"],
  options: { "ticket-system": { type: "Integer", description: "...", required: true,
               async autoComplete({ value, interaction }) { return { 1: "Ticket System #1" } } } },
  handle: async ({ interaction, locale, commandName }) => { ... }
});
// README: name: 'config (set|get|reset) (prefix|welcome)'  → 6 handlers-in-one; ctx.commandName tells which
```

How it works internally:
- **Pattern expansion.** `generateCombinations(pattern)` uses the regex `/\(([^)]+)\)(\??)/`. It does BFS over the first group in the string. `(a|b)` substitutes each alternative. A trailing `?` also adds the variant with the group removed. Whitespace is collapsed and results are de-duplicated via a Set.
- **Validation.** At least one combination is required. Every combination must have at most 3 words, and each word at most 32 chars (`CommandNameExceededMaxLengthError`). The `.d.ts` also enforces at most 3 words at the type level with template-literal types.
- **Dispatch.** The handler rebuilds `fullCommandName = commandName [+ " " + group] [+ " " + sub]` from the interaction. It then scans `cache.interactions` linearly for an entry whose `nameCombinations.includes(fullCommandName)`. Inspectors are the fallback. The README claims "single-map" dispatch, but the code is an O(n) scan.
- **Publish.**
  1. Every combination of every ChatInput is grouped by `(clientId, firstWord)`.
  2. Per base word: 1-part combinations become a top-level command. Its options are used **only** if there are no subcommands; Discord forbids mixing options and subcommands, and tescord silently ignores the top-level variant otherwise.
  3. 2-part combinations become `Subcommand`s, and 3-part become `SubcommandGroup → Subcommand`. When several combinations share a sub name, the first registration wins.
  4. The base command takes `defaultMemberPermissions`, `contexts` and `nsfw` from the **first** registration with that base word. Its description is the first non-empty one. Group descriptions copy the first member's description.
- dbi's predecessor model is the same idea without patterns. Each `ChatInput({ name: "a b c" })` is one handler. Publish sorts by word count descending, then builds or finds level-1 and level-2 nodes. Placeholder descriptions are `"..."`. Dispatch matches `name == [commandName, group, sub].join(" ")`.

### 3.4 Options (T map form; D builder form)

```ts
options: {
  user:   { type: "User", description: "The user", required: true },
  "cross-server": { type: "Boolean", description: "...", required: false },
  category: { type: "String", description: "...", choices: { music: "Music", videos: "Videos" } },
  limit:  { type: "Integer", description: "...", minValue: 1, maxValue: 99 },
  chan:   { type: "Channel", description: "...", channelTypes: ["GuildText"] },
}
```

- Supported types: `String | Integer | Number | Boolean | User | Channel | Role | Mentionable | Attachment`. Subcommand types are deliberately absent, because they come from the name pattern.
- Choices are `Record<value, displayName>`; the key is the value. Publish converts each entry to `{name, value}`, using `Number(key)` for Integer and Number options.
- `choices` and `autoComplete` are mutually exclusive at the type level.
- The map's insertion order is the option order. Required-before-optional ordering is not enforced by tescord, so Discord rejects a wrong order at publish time.
- Handlers read values with raw discord.js (`interaction.options.getUser("user")`). There is **no typed option extraction**, which is an obvious place for noctis to improve.
- dbi uses builders instead: `ChatInputOptions.string|stringChoices|stringAutocomplete|integer...|user|channel|role|mentionable|attachment|boolean({ name, description, required, validate?, onComplete? })`.

### 3.5 Autocomplete (T)

```ts
async autoComplete({ value, interaction }) {
  return Object.fromEntries((await prisma.payload.findMany({ where: { guild_id: interaction.guildId,
      name: { contains: value, mode: "insensitive" } }, take: 25 })).map(p => [p.id, p.name]));
}
```

How it works internally:
- The handler finds the ChatInput by combination, reads `options[focused.name].autoComplete`, and awaits the returned map.
- It converts each entry to `{name, value}` (coercing the value by option type), truncates to 25, then calls `respond`.
- On an error it emits `tescord:autocompleteError` and responds with `[]`.
- If there is no handler it also responds `[]`.

### 3.6 Permissions, contexts, nsfw (T) / directMessages (D)

- `defaultMemberPermissions: ["ViewAuditLog"]` is OR-ed into a bitfield string. `null` when empty.
- `contexts: ["Guild" | "BotDM" | "PrivateChannel"]` maps to `InteractionContextType`. `nsfw: boolean`.
- tescord does not expose `integration_types`. noctis should add it (guild install / user install).
- dbi uses `directMessages` (the deprecated `dm_permission`) and a global default `defaults.defaultMemberPermissions`. dbi's message commands also *enforce* these permissions client-side, because prefix commands have no Discord-side gate.

### 3.7 Context menus (T)

```ts
pack.userContextMenu({ id: "user-info", name: "Get User Info", handle: async (ctx) => ctx.interaction.targetUser });
pack.messageContextMenu({ id, name, handle, clientId? });
```

These dispatch by an exact `name` match. Their i18n entry is `{ name, description? }`.

### 3.8 Component registry and `buildComponent` (T) / `toJSON` (D)

```ts
pack.button({ id: "punish-history-set", options: { style: "Secondary" },
  handle: async ({ interaction, data, locale }) => { const [userId, guildId, page] = data; ... } });

tescord.buildComponent({ id: "punish-history-set",
  data: [target.userId, target.guildId || 0, page + 1],
  overrides: { disabled: punishments.length < 25, label: locale.punishHistory.buttons.nextPage() } });

await interaction.showModal(tescord.buildComponent({ id: "temp_voice_name_modal", data: [voiceProfileId],
  overrides: { title: locale.guild.tempVoice.manager.modals.changeChannelName(),
               components: [{ components: [{ label: "...", placeholder: "...", value: profile.name }] }] } }));
```

How it works internally:
- `buildComponent` looks up `cache.interactions.get(id)` and fails if it is missing.
- It then deep-merges overrides onto the registered `options` with `stuffs.defaultify(overrides, defaults, true)`. Arrays merge **index-wise**, which is why a modal override can supply only `label/placeholder/value` for row 0 / input 0 and inherit `customId`, `style` and `type` from the registration.
- It maps style names to enums and then emits discord.js component JSON. Defaults: `minValues/maxValues = 1`, `disabled = false`, modal `title = "Modal"`. A `url` replaces `customId`, which gives Link buttons.
- Supported kinds: Button, StringSelect, UserSelect, RoleSelect, ChannelSelect, MentionableSelect, Modal.
- Emoji can be a unicode string or a custom-emoji id string. schwi-v2 uses `get emoji() { return emojis.voice.name }` getters for lazy dev/prod ids.
- `buildComponentAsync` uses the async codec.
- schwi-v2 convention: multi-step flows use suffixed ids like `ticket-system-edit-text` (button) → `ticket-system-edit-text:1` (modal) and `seacord-ability-role:0` → `:1`. The `:` is only a naming convention.
- dbi: `dbi.interaction("buy-item").toJSON({ overrides, reference: { data: [...], ttl? } })`, plus a `createBuilder()` fluent builder per component type.

### 3.9 Generated types (T + `@tescord/type-gen`)

- `src/types/index.ts` calls `writeLocalizationTypes({ tescords:[tescord], outputPath })` and `writeComponentTypes(...)`. A `nodemon` watch runs it as `pnpm types`.
- It generates `declare global { namespace Tescord { interface ComponentMap { 'punish-history-set': 'Button'; ... } } }` and `interface Localization { punishHistory: { header: (_0: string, _1: string, _2: string) => string } }`.
- The parameter count comes from `{n}` placeholders in the *raw* template strings, which it reads from the private `Locale.contents`. Named parameters `{0:user}` become `user: string`.
- Users can also extend `Tescord.CustomDataTypes` to widen what `data` may contain.
- `todo.md` notes that the goal is "generations modifiable from anywhere with declare module extending".
- **Rust mapping:** a `build.rs` or proc-macro reads the i18n YAML and generates a typed `Locale` struct tree with methods and arity. Component ids become an enum or typed handles produced by registration macros, so no codegen step is needed at all.

### 3.10 Content i18n (T)

```yaml
# en.content.yaml
payload:
  editor:
    delete:
      success: "`{0}` successfully deleted."
```
```ts
locale.loadFile({ id: "en/content", filePath, language: "en", path: "$", type: "Content" });
locale.addLocale({ id: "welcome", locale: "en", data: { title: "Welcome!" } });
// usage
locale.user.payload.editor.delete.success(name)
```

How it works internally:
- Files may be `.json`, `.yaml` or `.yml`. `path` is a tiny JSONPath (`$` or `$.a.b`).
- Per language, all sources are deep-merged, then converted so every string leaf becomes a function `(...args) => text.replace("{i}", arg)`. Note that `String.replace` with a string pattern replaces **only the first occurrence** of each placeholder.
- On `tescord.refresh()`, locales from all packs are merged per language. Then every non-default language is `defaultify`-ed with the default language, so missing keys fall back to `defaults.language`.
- Language keys are the base code (`en`, `tr`, `pt`). Discord's `pt-BR` and similar become `pt` via `split('-')[0]`.
- dbi uses an *infinite path Proxy* (`locale.user.data.a.b.c(args)`). On a missing path it falls back to the default locale, then to `defaults.locale.invalidPath({path, locale})`. It replaces with regex `\{i(;[^}]+)?\}` globally, which also supports `{0;comment}` syntax.

### 3.11 Interaction i18n (T)

```yaml
# tr.interactions.yaml   (key = interaction id)
send-ticket-create-message:
  names: { send: gönder, ticket-create: bilet-oluştur, message: mesaj }   # word-by-word
  description: "Bilet oluşturma mesajını göndermek için"
  options:
    ticket-system: { name: bilet-sistemi, description: "...", choices: { welcome: "Karşılama" } }
```

How it works internally:
- At publish, for every loaded language, tescord finds the matching Discord locales (`tr` → `tr`; `en` → `en-GB`, `en-US`; `es` → `es-ES`, `es-419`).
- It sets `name_localizations` per level by translating each word of the combination through `names`.
- It also sets `description_localizations`, option `name/description` localizations, and choice `name_localizations`, where the choice key is the value.
- Context menus use `{ name, description? }`.
- dbi's `InteractionLocale({ name: "a b c", data: { tr: { name: "x y z", description, options } } })` translates the whole name and picks word *i* per level. dbi's message commands also accept the *translated* name as a prefix command.
- schwi-v2 adds a dev-time scaffold: when running from `.ts`, it walks every ChatInput and writes `src/i18n/example/interactions.yaml` with TODO entries, keeping creation order in `order.json`.

### 3.12 Locale on context (T)

```ts
handle: async ({ interaction, locale }) => locale.user.x()   // interactions: user + guild
pack.event({ event: "voiceStateUpdate", handle: async ({ oldState, newState, locale }) => locale.guild.y() })
```

How it works internally:
- `guildLocale = guild.preferredLocale.split('-')[0] || default`, and `userLocale = interaction.locale.split('-')[0] || guildLocale`.
- Each resolves to the merged content tree, or to the default tree if that language is absent.
- For events, the guild is extracted heuristically from named args (`guild`, `args[0].guild`, `member.guild`, `message.guild`, `interaction.guild`). `locale.user` uses the interaction's locale if there is one, and otherwise equals the guild locale.
- schwi-v2's rule: use `locale.user` for ephemeral, single-user UI; use `locale.guild` for content many people see.

### 3.13 Gateway event handlers (T)

```ts
trigger.event({ event: "guildAuditLogEntryCreate", handle: async ({ guild, auditLogEntry }) => { ... } });
pack.event({ event: "ready", handle: async ({ client }) => { ... } });
pack.event({ event: "messageCreate", handle: async ({ message, locale }) => { ... } });
```

How it works internally:
- `build.ts` scrapes discord.js `ClientEvents` typings into `TescordClientEventMap`, which maps an event name to its parameter names (for example `voiceStateUpdate: ["oldState","newState"]`).
- At dispatch, positional args are zipped into a named context object, and locale is added.
- Then `beforeEvent` middleware runs. For each matching registered event, `beforeEventHandle` runs first. The handler is **awaited sequentially**.
- Finally the context is also emitted on the pack event bus under the raw event name.
- Event ids are `${packId}:${event}:${Date.now()}`. Two registrations in the same millisecond collide, which is a bug noctis must avoid.
- dbi does the same zipping via `eventMap.json`. On top of that it has `ordered: { await, delayBefore, delayAfter }`, `disabled`, `triggerType: "OneByOne" | "OneByOneGlobal" | "Random"` (which client to pass as `nextClient`, for multi-bot load spreading), `CustomEvent`s via `dbi.emit(name, args)`, and `afterEvent`.

### 3.14 Middleware and the result-collecting event bus (T)

```ts
tescord.events.on("beforeEvent", async ({ guild }) => {
  if (await cache.customBotStatus.getCustomBotExists(guild.id)) return false;  // cancel: a custom bot owns this guild
});
tescord.events.on("tescord:interactionHandlerError", e => console.error(e));
```

How it works internally:
- `ResultEventEmitter` is an emitter whose `emit` returns listener results. It offers `emitAsync` (an async iterator, sequential), `emitParallel`, and `emitUntilResult[Async]`, which stops at the first non-undefined result.
- `Pack.events` propagates every emit to all sub-packs recursively.
- `beforeInteraction` / `beforeEvent`: a listener returning `false` cancels; a listener returning an object merges it into the context. In tescord 0.0.6 that merged `extraContext` is never actually passed to interaction handlers, which is a bug.
- dbi's `Events.trigger` requires handlers to return `true` to continue (fail-closed). Its event list is `beforeInteraction`, `afterInteraction`, `interactionRateLimit`, `beforeEvent`, `afterEvent`, `interactionError`, `eventError`, and the message-command error events.
- Lifecycle events (T): `pack:loaded|unloaded|destroyed`, `interaction:registered|unregistered`, `locale:loaded|unloaded`, `inspector:registered|unregistered`, `tescord:clientReady|clientsReady|clientDestroy|clientEvent`, `<clientId>:<event>`, `tescord:cacheRefreshed`, `tescord:localesRefreshed`, `tescord:interactionsPublished|interactionsPublishError`, and `tescord:customData:encode|decode`.

### 3.15 Inspectors (T)

```ts
const inspector = new Inspector({ id: "CommandInspector", domain: "CurrentPack|AllSubPacks" });
inspector.chatInput({ pattern: "system (start|stop) (server|database)", handle });
inspector.button({ id: "dynamic-button-*", handle });   // README claims wildcard; code does exact Map lookup
pack.use(inspector);
inspector.emit({ type: "chatInput", id: "system start server", ctx });   // manual invocation
```

Inspectors are secondary routers that run when no registered interaction matched. `domain` is stored but unused, and wildcards are not implemented. schwi-v2 does not use them. For noctis, wildcard/prefix component routing is worth providing as a real feature (P2).

### 3.16 Multi-client (T) and sharding (D, S)

- In tescord, `clients[]` each have an `id`. A command's `clientId` routes it to that client at publish time; an unknown id warns and falls back to the first client. All clients share one handler tree. `ctx.client` is the `TescordClient {id, client, token}`.
- In dbi, `discord: [{ namespace, token, options }]` and `publish: "ns"` per interaction. `dbi.client(name?)`, `clients.next(key)` (round-robin), `.random()` and `.first()` exist. Sharding is `"hybrid"` (discord-hybrid-sharding `ClusterClient`), `"default"` or `"off"`, and multi-client is incompatible with sharding.
- schwi-v2:
  - `index.ts` runs a `ShardingManager(mode: "process")`. `bot.ts` is the per-shard entry.
  - `startCustomBot(token, guildId, features)` spawns a separate process for a customer's own bot token, passing `--custom --guild --features`.
  - A `beforeEvent` gate makes the main bot ignore guilds that have an enabled custom bot (cached in Redis). The custom bot in turn ignores every guild except its own.
  - Cross-shard lookups use `client.shard.broadcastEval` (`utils/shardMethods.ts`).
  - Each shard connects to an API WebSocket (`/shards-ws`) to push guild, channel and role state.

### 3.17 Publishing (T)

```ts
// bot.ts
if (args.publish) tescord.publish(guildId).then(() => process.exit(0)); else tescord.start();
// package.json: "commands": "tsx src/bot.ts --publish"
```

`publish(guildId?)` does `refreshClients()` and `refresh()`, builds the per-client body, then for each client calls `GET /users/@me` to get the application id and performs a bulk `PUT` to the guild or global commands route. It emits `tescord:interactionsPublished`. dbi adds `publish("Global"|"Guild", guildId?, clear?)`, where `clear` sends an empty array.

### 3.18 Rate limits (D only)

```js
ChatInput({ name: "daily", rateLimits: [{ type: "User", duration: 86_400_000 }], onExecute({ setRateLimit }) { ... } })
dbi.events.on("interactionRateLimit", ({ interaction, rateLimit }) => { interaction.reply("Slow down"); return true; })
```

How it works internally:
- The keys are `RateLimit["<name>_<userId>"]` for User, plus `Channel`, `Guild`, `Member` (user + guild) and `Message` variants, stored in `config.store`.
- Before execution, every key type is checked. An expired entry is deleted. An active one triggers `interactionRateLimit`, and if a listener returns `true` the interaction is swallowed.
- Then the interaction's declared `rateLimits` are set, and `ctx.setRateLimit(type, ms)` allows dynamic limits from inside the handler.
- schwi-v2 hand-rolls this twice: an in-memory sliding window for channel renames (Discord's 2 per 10 min), and Redis fixed-window counters in `packages/cache/rateLimit.ts`.

### 3.19 Feature flags (D) vs schwi-v2's hand-rolled flags

dbi: any registration can have `flag: "debug"`, and `dbi.load("debug")` or `load("all")` selects them. Unflagged items always load. schwi-v2 instead uses a yargs `--features a,b` / `--seacord` CLI with `features.has(x)`, which wraps `pack.use(...)` and individual `slashCommand` calls in `if`s. noctis should support this declaratively with a `.flag("seacord")` on registrations or packs, selected at startup.

### 3.20 Prefix message commands (D)

`messageCommands: { prefixes: ["!"] | (ctx) => [...], typeAliases: { booleans: { yes: true } } }`.

How it works internally:
- dbi matches `content.startsWith(prefix + chatInput.name)`, or an alias in `other.messageCommand.aliases`, or a *translated* name.
- It then parses arguments by option order with type coercion and validation, including autocomplete-backed choice checking.
- It builds a `FakeMessageInteraction` whose `reply`, `deferReply`, `editReply` and `followUp` map onto message replies, so the **same slash handler serves both**.
- Errors go to events such as `messageCommandArgumentError`, `messageCommandDefaultMemberPermissionsError` and `messageCommandDirectMessageUsageError`.
- schwi-v2 builds its own `s!` prefix custom-command system (user-scripted via `hexput`), which is unrelated to slash emulation.

### 3.21 Inline listeners (D)

`createInlineButton({ onExecute, ttl? })` registers an anonymous component with the id `inline:<random>`. It can be rendered via `.toJSON()` and is auto-removed by a sweeper after `ttl`; the default is a check every 60 s and a 15 min TTL. The same exists for selects, modals and events. This is great for one-off confirm dialogs; noctis could offer it as closures stored in an in-memory map with TTL.

### 3.22 Components V2 (S; D's HTML variant)

- schwi-v2 uses **only** Components V2 (`flags: IsComponentsV2`, with Container / TextDisplay / Section / MediaGallery / Separator / ActionRow), built as raw discord.js JSON objects. tescord's built components are slotted into ActionRows inside Containers.
- `components_v2.xml` (an AI-oriented schema of component types and limits) and `components.txt` (a scrape of the Discord docs) are reference material for authoring.
- The "payload" integration is a full WYSIWYG editor for stored V2 messages with **placeholders**. `prisma.payload.resolvePayload(id, { create_ticket: { type: "button", component: tescord.buildComponent(...) } })` injects framework-built components into user-designed layouts.
- dbi goes further with `HTMLComponentsV2({ mode: "svelte" | "eta", file | template })`, which renders Svelte 5 or Eta templates to V2 JSON. Its custom ids are prefixed `v2:` and the element name is the first data item. It keeps reactive `data` held by reference ($ref), offers `send()` and `render()`, and tracks pending modals.

### 3.23 Store / caching (D trait; S Redis)

- dbi's `DBIStore { get(key, default?), set(key, value), delete(key), has(key) }` defaults to `MemoryStore`, which is lodash-path based and not persistent. dbi uses it for rate limits.
- schwi-v2's `packages/cache` is a set of hand-written Redis modules using RedisJSON + RediSearch indexes (for example `lateDeleteChannel` with `FT.SEARCH` numeric ranges). It covers:
  - welcome dedupe
  - DM↔forum message mapping
  - custom-bot status
  - two-way partnership
  - channel and role mirrors
  - rate-limit counters

  noctis should ship a typed KV/cache trait (memory + Redis adapters) used both by framework features (refs, rate limits) and by apps.

### 3.24 Images (S)

`packages/satori` exposes `registerFonts(...)` and `render(jsx, { width, height, fonts, background })`. The pipeline is satori (JSX→SVG) → resvg (PNG) → sharp (WebP). It is not wired into the bot yet (`welcome/design.ts` is `// todo`). A Rust equivalent would be `resvg` + `tiny-skia`, with an HTML/flexbox layout via `taffy`. This should be a separate optional crate (`noctis-image`), not core.

---

## 4. Deep dive: custom-id and reference mechanisms

Discord custom_id: max **100 chars** (UTF-16 code units count per Discord docs; treat it as 100 characters). It is returned verbatim on button, select and modal interactions.

### 4.1 tescord (current, minimal)

| Element | Value |
|---|---|
| Separator | `䲜` (U+4C9C) between the base id and each datum |
| Number tag | `㥻` (U+397B) prefix, e.g. `㥻42` |
| Strings | raw |
| Other types | `JSON.stringify` fallback, but this is **not** reversed on decode (it comes back as a string) |
| Layout | `"<id>䲜<d1>䲜㥻<n2>…"`; with no data the custom_id is just `<id>` |
| Decode | `split("䲜")` → `[id, ...parts]`; `㥻`-prefixed parts become `Number`, everything else stays a string |
| Extensibility | Before the default codec runs, `tescord:customData:encode` receives `{ notParsed: values[], parsed: string[] }`. Listeners move items they can handle from `notParsed` into encoded `parsed` strings. Decode mirrors this. Sync and async versions exist; the async one runs listeners sequentially. |
| Length check | **None**. Over-length ids fail at the Discord API. The splitter inside user strings is not escaped. |
| Storage / TTL | None built in. Users would implement references through the codec hook. |

Example from schwi-v2: `data: [userId, guildId || 0, page + 1]` → `punish-history-set䲜123456789012345678䲜㥻0䲜㥻2`. Payload-editor ids carry `[payloadId, componentId, "text-display"]`. All state is kept as **small primary keys in custom ids, with the real state in Postgres**. This is the dominant pattern.

### 4.2 dbi (richer; the reference model noctis should generalise)

`buildCustomId(dbi, name, data, ttl, v2)`:

| Element | Encoding |
|---|---|
| Separator | `—` (U+2014 EM DASH) |
| V2 marker | `v2:` prefix on the name (HTMLComponentsV2) |
| string | raw |
| number | `π<n>` |
| bigint | `ᙖ<n>` |
| boolean | `𝞫1` / `𝞫0` |
| undefined / null | `🗶u` / `🗶n` |
| object (already referenced) | `¤<refId>` |
| object (new) | 8-char random id; `value.$ref = id`, `value.$unRef = () => refs.delete(id)`; `dbi.data.refs.set(id, { at: Date.now(), value, ttl })`; encoded as `¤<id>` |

- Length rule: in `strict` mode, an id longer than 100 chars **throws**. In non-strict mode it is **silently truncated** to 100.
- Decode: split on `—`, then dispatch on the prefix char. `¤id` → `refs.get(id)?.value`, which is `undefined` once expired.
- Ref storage: an in-process `Map` (`config.data.refs` can be injected at construction, so you can pass a Map-like object).
- Expiry: `references.autoClear = { check, ttl }`. A `setInterval(check)` deletes entries where `Date.now() > at + (entry.ttl || autoClear.ttl)`. It is off by default (`autoClear: undefined`), in which case refs live forever, which is a leak. A per-call TTL comes from `toJSON({ reference: { data, ttl } })`.
- Ref ids are not namespaced per client/shard and are not persistent: a restart loses every ref, and buttons pointing at them get `undefined` data. Handlers must cope.
- Inline listeners use the same TTL machinery (`inlineListeners.autoClear = { check: 60_000, ttl: 900_000 }` by default).

### 4.3 Requirements derived for noctis's storage adapter

1. **Codec layer (pure, sync).** A typed `CustomData` encoding with:
   - a versioned header byte
   - an escape scheme (or length-prefixing) so separators inside strings are safe
   - compact numerics (base-62 or varint→base-N for snowflakes, bools, enums)
   - **compile-time or build-time 100-char validation** where possible, with a runtime check plus a configurable overflow policy (`Error` | `SpillToStore`)
2. **Reference layer (async, pluggable).** It handles values that do not fit or are not serialisable inline. Something like:
   ```rust
   #[async_trait] pub trait RefStore: Send + Sync {
       async fn put(&self, key: &RefKey, bytes: Bytes, ttl: Option<Duration>) -> Result<()>;
       async fn get(&self, key: &RefKey) -> Result<Option<Bytes>>;
       async fn delete(&self, key: &RefKey) -> Result<()>;
       async fn touch(&self, key: &RefKey, ttl: Duration) -> Result<()> { Ok(()) } // sliding expiry
   }
   ```
   - Adapters: `MemoryRefStore` (moka/DashMap + TTL sweeper), `RedisRefStore` (`SET PX`, natural cross-shard sharing), and optionally SQL.
   - The custom_id stores `¤`-style short keys: random base-62, 8 to 12 chars, optionally namespaced `{app}:{key}`.
   - Values are serialised with `serde` (bincode/postcard/MessagePack). A ref must also survive *restarts* when backed by Redis, which fixes dbi's biggest weakness.
3. **Explicit expiry semantics.**
   - Per-component TTL with a framework default.
   - Expired or missing refs produce a typed error (`RefExpired`) that routes to an overridable "this interaction has expired" handler, rather than silent `undefined`.
   - `unref()` / delete-on-use for one-shot flows (dbi's `$unRef`).
4. **Hook parity.** Keep tescord's encode/decode interception as a trait (`CustomDataCodec`) so apps can add types, for example "Prisma id → entity".
5. **Same store trait reused** for rate limits, inline-listener registry metadata, and app caches. dbi does this with `config.store`.

---

## 5. schwi-v2 usage patterns worth copying

1. **One Pack per module, split by role.** `modules/<name>/{index,setup,trigger,commands}.ts`: `setup` = admin slash command + setup UI components, `trigger` = gateway events, `index` = `pack.use(setup); pack.use(trigger)`. Pack ids are path-like (`modules/punish-history/commands`).
2. **Render functions that return V2 component trees**, reused by the slash command (initial render) and by every button, select or modal handler (re-render after mutation via `deferUpdate` + `editReply`). Example: `renderSetupMenu(client, locale.user, guildId, systemId)`, `getPunishmentMessageComponents(target, locale, page)`. This is the "stateless UI = f(DB state, ids in custom_id)" pattern.
3. **Pagination by re-encoding page numbers** in custom-id data: `data: [userId, guildId || 0, page ± 1]`, with `disabled` computed from bounds. There is no pagination helper; noctis could offer one.
4. **Button → modal → re-render chains** with suffixed ids (`x` → `x:1`), forwarding `data` unchanged, and pre-filling modal inputs via index-wise override merge.
5. **All user-visible strings via typed locale accessors.** Placeholders are positional (`{0}`). Mentions are composed in code and passed in as args.
6. **Dev/prod emoji id tables** selected at startup and referenced lazily from component options.
7. **Global middleware for tenancy and gating** (`beforeEvent` returning `false`), with Redis-cached lookups.
8. **Publish as a CLI mode of the same binary** (`--publish [--guildId]`), and guild publish for the dedicated server feature set.
9. **Feature selection via CLI flags** (`--features all|seacord,...`, `--custom`, `--token`, `--guild`) so one codebase runs as the main sharded bot, a dedicated-server bot, or a per-customer white-label bot.
10. **Autocomplete backed directly by DB queries**, returning `{id: label}` maps capped at 25.
11. **Generated types kept in-repo** and regenerated by a watcher. In Rust this becomes a compile-time macro, so no watcher is needed.
12. **Translation scaffolding** generated from registered commands (`i18n/example/interactions.yaml`).

Anti-patterns or gaps observed (and what noctis should do instead):
- `as number` / `as string` casts on `data[i]` → **typed data tuples/structs** per component.
- Untyped `interaction.options.getX("name")` → **typed option structs** derived from declarations.
- Hard-coded "Todo localization" strings in modals → have the framework warn on missing translations.
- Copy-pasted rate-limit code → built-in rate-limit middleware.
- `as any` on overrides for selects and `beforeEvent` → typed overrides and typed middleware ctx.

---

## 6. Recommendations for a Rust equivalent (noctis)

1. **Core shape: an `App` root that is itself a `Pack`.** Packs are values that you compose with `app.mount(pack)` or `pack.mount(sub)`. Every registration returns a `Registration` handle that is RAII or explicitly `dispose()`-able. Keep a flattened, **truly O(1)** routing index rebuilt on mutation:
   - `HashMap<CommandPath, HandlerId>`
   - `HashMap<ComponentId, HandlerId>`
   - `HashMap<EventKind, Vec<HandlerId>>`

   Swap the index atomically (`ArcSwap`) so hot unload and reload is lock-free for dispatch.
2. **Commands.** Support tescord's pattern names (`"config (set|get) (prefix|welcome)?"`). Expand them at registration with the same BFS semantics, validate (at most 3 words, 1 to 32 chars, lowercase, `^[-_\p{L}\p{N}]{1,32}$`), and **error rather than silently drop** when a base command mixes top-level options with subcommands. Merge across packs by base word. Define a precedence rule for base-level `default_member_permissions`, `contexts` and `integration_types`, such as "must agree or error", instead of "first wins". Expose the matched combination as a typed enum where possible.
3. **Typed options.** Use a `#[derive(CommandOptions)]` struct whose fields carry `#[option(description=..., min=.., choices=..)]` attributes. The derive yields both the publish JSON and a parser from twilight's `CommandData`. Keep a map-style builder API too, for dynamic cases. Autocomplete is an async closure or trait method on the field returning `Vec<(value, label)>`, auto-truncated to 25.
4. **Components.** `#[derive(Component)] struct PunishHistorySet { user: Id<UserMarker>, guild: Option<Id<GuildMarker>>, page: u32 }`:
   - Registering the handler fixes the id and default options.
   - `PunishHistorySet { .. }.button().label(..).disabled(..)` builds the twilight model.
   - The handler receives the decoded struct.

   This replaces both `buildComponent` + `ComponentMap` codegen and the `data[i] as T` casts. Keep a JSON-patch-like `overrides` merge for modal pre-fill.
5. **Custom-id codec plus pluggable `RefStore`** as specified in §4.3: inline compact encoding first, spill to the store when needed or when a field is `#[component(by_ref)]`, TTL per component, and a typed "expired" path. Provide memory and Redis adapters behind feature flags.
6. **i18n.**
   - YAML/JSON content files go through a proc-macro (`noctis::i18n!("locales")`) or `build.rs` into a typed `Locale` tree. Leaves become methods with arity from `{0}` / `{0:name}` placeholders, and **all occurrences** are replaced.
   - Missing keys fall back to the default language at build time, with warnings.
   - Interaction locale files translate names word by word per level, plus descriptions, options and choices. Map base language codes to the Discord locale variants exactly like tescord.
   - Every context exposes `ctx.locale.user()` / `ctx.locale.guild()`.
   - Ship a `noctis i18n scaffold` CLI that emits a translation template from registered commands.
7. **Events.** Provide typed per-event handlers over twilight `Event` variants (`pack.on::<VoiceStateUpdate>(|ctx, ev| ..)`), with guild/user/locale extraction helpers. Define ordering semantics (concurrent by default, `ordered`/awaited opt-in, as in dbi). Handler ids must be unique (not timestamp-based).
8. **Middleware.** A tower-like or simple async chain: `before_interaction` and `before_event` return `Flow::Continue(ctx_ext) | Flow::Stop`. Add `after_*` hooks and error sinks (`on_interaction_error`, `on_event_error`, `on_autocomplete_error`). Typed context extensions (`ctx.ext::<T>()`, like `http::Extensions`) replace tescord's untyped object merge, and actually reach handlers.
9. **Built-in opt-in modules (from dbi):**
   - **rate limits** (User/Channel/Guild/Member/Message scopes, declarative `.rate_limit(Scope::User, 10s)` plus a dynamic `ctx.set_rate_limit`, backed by the store trait)
   - **feature flags** (`.flag("seacord")`, selected at boot)
   - **inline/ephemeral components** (closures with TTL)
   - **prefix message commands** reusing slash handlers via an abstract `Responder` trait implemented for both interactions and messages
   - option **validators**
10. **Multi-client and sharding.**
    - Multiple twilight `Shard` groups and `HttpClient`s keyed by client id, with commands targeting a `client_id` and publish per client.
    - Sharding via twilight-gateway's `create_recommended` / bucketed shards inside one process (no process-per-shard needed). Optionally a cluster id/range for multi-process deployments, plus a cross-shard request/response bus (Redis pub/sub) to replace `broadcastEval`.
    - Support schwi-v2's "white-label custom bot" model: run N independent `App` instances with different tokens and a guild filter, ideally in one process.
11. **Publish.** `app.publish(Scope::Global | Scope::Guild(id))` does a bulk overwrite per client. Add a **diff/dry-run** mode (print JSON, compare with remote) and a CLI subcommand (`--publish`), mirroring schwi-v2's workflow.
12. **Components V2 first-class.** Typed builders for Container, TextDisplay, Section (with accessory), Thumbnail, MediaGallery, File, Separator and ActionRow. Enforce the V2 limits from `components_v2.xml` (nesting rules, 40 total components, text lengths) at build time and in debug assertions. Provide a default `IS_COMPONENTS_V2` reply helper. A templating layer (dbi's Svelte/Eta idea) is P2; Rust macros (`v2! { container { text!("..") } }`) are the idiomatic analogue.
13. **Cache/KV trait** shared by framework and app (`get/set/delete/has` + TTL), with memory and Redis adapters. Optionally typed "repositories" like schwi-v2's `cache.lateDeleteChannel`, but those stay app-level.
14. **Images:** keep them out of core. Offer a `noctis-image` crate later (resvg/tiny-skia + font registry + WebP encode) mirroring `@misket/satori`'s `registerFonts` + `render`.
15. **Fix tescord's known bugs by design:**
    - O(n) dispatch scans
    - `beforeInteraction` context never reaching handlers
    - timestamp-based event ids
    - first-occurrence-only placeholder replacement
    - un-escaped separators, missing 100-char checks, and the lossy JSON fallback in the codec
    - an unused Inspector `domain` and unimplemented wildcard matching
    - silent dropping of top-level options when subcommands exist
    - "first registration wins" for merged command metadata
    - missing `integration_types`

---

### Appendix A: source files consulted
- tescord: `dist/lib/{Tescord,Pack,Inspector,Locale}.js`, `dist/lib/utils/{pattern,handleInteraction,handleEvent,publishInteractions}.js`, `dist/lib/types/{ComponentBuilder,ResultEventEmitter}.js`, `dist/lib/types/*.d.ts`, `README.md`, `.github/copilot-instructions.md`, `todo.md`, `build.ts`, `commands.json`.
- @tescord/type-gen 0.0.5: `dist/index.js`.
- dbi 0.2.37: `readme.md`, `dist/src/DBI.{js,d.ts}`, `dist/src/Events.js`, `dist/src/utils/{customId,MemoryStore}.js`, `dist/src/methods/{hookInteractionListeners,hookEventListeners,publishInteractions,handleMessageCommands}.js`, `dist/src/types/{Interaction, Components/Button, other/Locale, other/InteractionLocale, Components/HTMLComponentsV2/index}.*`.
- schwi-v2: `apps/discord-bot/{package.json, .github/copilot-instructions.md, components_v2.xml, components.txt, docs/WS_PROTOCOL.md}`, `src/{tescord,bot,index,features,env,global.d}.ts`, `src/i18n/*`, `src/types/*`, `src/modules/{index, punishHistory/*, ticket/setup, temp-voice/trigger, custom-commands/trigger}.ts`, `src/integrations/{index, ws/channels, payload/*, emoji}.ts`, `src/utils/shardMethods.ts`, `packages/satori/src/*`, `packages/cache/src/{index,redis,caches/index,caches/rateLimit,caches/lateDeleteChannel}.ts`.
- Extracted package copies (read-only): `/tmp/claude-0/-home-user/dd8af539-19a8-5b5d-b835-27c3a80d0c45/scratchpad/research-dbi/{tescord,dbi,typegen}/package/`.
