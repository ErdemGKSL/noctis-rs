# Discord Rules Catalog

**Status:** reference for Noctis implementers. Every Discord API rule or limit that
Noctis must respect is listed here, mapped to the point where Noctis enforces it.
**Authority:** where this catalog and the architecture spine
(`_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md`)
disagree, the spine wins and this file is corrected. The `custom_id` layout and its
constants (AD-5, AD-6) are quoted here verbatim, and `xtask rules-catalog` checks them.
**As of:** 2026-10-19. Verified against `discord/discord-api-docs` @ `f7416a2`
(2026-10-16) and the research notes in
`_bmad-output/planning-artifacts/research/` (`discord-components-v2.md`,
`tescord-features.md`, `twilight-rs.md`).

## How to read this catalog

### Enforcement tiers

Noctis checks every rule at the **earliest point where all the inputs are known**:

| Tier | When | Applies when | Failure looks like |
|---|---|---|---|
| **Compile-time** | `cargo check` / `cargo build` | The input is static: string literals in attributes or macros, field types of a `derive`d struct, const generics, the shape of a typed component tree. | `compile_error!` from a proc macro, a failed `const` assertion, or a missing trait impl (type error). |
| **App-build** | `App::builder()...build()`, before any gateway connection or HTTP write | The rule depends on combining Modules (merging Commands across Modules, global counts, Route uniqueness, intents versus handlers). | `Err(BuildError)` that lists **all** violations (not just the first), each naming the rule ID, the Modules involved and the actual value. `--publish --dry-run` runs the same checks. |
| **Runtime** | While handling an interaction or sending a request | The data is genuinely dynamic: user input, database rows, `Vec`s of unknown length, timers, Discord-side quotas. | A typed `Error` returned before the request is sent (never a 400 from Discord, never a panic). Some rules have a lenient mode (truncate with `…`) as FR-24 allows. |

A rule often has several tiers. For example, a button label written as a literal is
checked at compile time, and a label built from a database row is checked at runtime by
the same validator code. The **Enforcement** column lists the tiers in the order they
apply. "Compile-time (literal) / Runtime (dynamic)" means the macro checks literals
and the runtime validator checks everything else.

### Confidence markers

- **[D]**: stated in Discord's official docs (linked).
- **[C]**: observed by the community or other libraries (discord.js, JDA, Discord.Net),
  enforced by the API but not written in Discord's docs.
- **[U]**: **unconfirmed exact value or counting unit.** Noctis uses the conservative
  interpretation and keeps the value configurable. See
  [Unconfirmed values](#unconfirmed-values).
- **[N]**: a Noctis design choice, not a Discord rule.

### Rule IDs

Every rule has a stable ID (for example `CMD-03`). Compile errors, `BuildError`
variants, runtime errors and trybuild fixtures all cite this ID, so a failing build
points straight back to this file.

### Source abbreviations

| Abbrev. | Page |
|---|---|
| AC | [Application Commands](https://discord.com/developers/docs/interactions/application-commands) |
| RR | [Receiving and Responding](https://discord.com/developers/docs/interactions/receiving-and-responding) |
| CR | [Component Reference](https://discord.com/developers/docs/components/reference) |
| MSG | [Message resource](https://discord.com/developers/docs/resources/message) |
| WH | [Webhook resource](https://discord.com/developers/docs/resources/webhook) |
| REF | [API Reference](https://discord.com/developers/docs/reference) |
| RL | [Rate Limits](https://discord.com/developers/docs/topics/rate-limits) |
| GW | [Gateway](https://discord.com/developers/docs/events/gateway) |
| GWE | [Gateway Events](https://discord.com/developers/docs/events/gateway-events) |

---

## 1. Chat-input command names and descriptions

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CMD-01** Name charset | Must match `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` with the Unicode flag. Note: the official regex allows `ʼ` (U+02BC, modifier letter apostrophe), **not** the ASCII `'`. Applies to command, subcommand group, subcommand and option names. **[D]** | [AC §Naming](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-naming) | Compile-time (literal) / App-build (names from runtime-loaded Locale files) | `#[command("…")]` / `command!` splits the Command Pattern into words and matches each against the regex. The check lives in `noctis-rules` (which may depend on `regex` with Unicode tables); the proc macro and App-build call the same function. Error spans point at the offending word. |
| **CMD-02** Name length | 1-32 characters per word **[D]** (counting unit **[U]**: Noctis counts both Unicode scalar values and UTF-16 units and takes the larger) | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (literal) / App-build (loaded locales) | Same macro pass as CMD-01. Empty words (`"config  set"`) are rejected. |
| **CMD-03** Lowercase | If a letter has a lowercase variant, the lowercase form must be used. Uncased letters (e.g. CJK, Devanagari) are allowed. **[D]** | [AC §Naming](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-naming) | Compile-time (literal) / App-build (loaded locales) | Macro checks `c.to_lowercase() == c` for every `char`. The error suggests the lowercased word. |
| **CMD-04** Description length | 1-100 characters for `CHAT_INPUT` commands, subcommands, groups and options **[D]** | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (literal, incl. doc comments used as descriptions) / App-build (loaded locales) | `description = "…"` or `///` doc comments are measured in the macro. A missing description is a compile error (Noctis never sends tescord's `"..."` placeholder **[N]**). |
| **CMD-05** Per-command size budget | Combined `name` + `description` + choice `value` of the command, all options (incl. subcommands and groups) and all choices ≤ **8000** characters. With localizations, only the **longest** variant per field (including the default) counts. **[D]** (counting unit **[U]**) | [AC §Slash Commands](https://discord.com/developers/docs/interactions/application-commands#slash-commands) | Compile-time (one pattern, static locales) / App-build (after merging Modules, or with runtime-loaded locales) | Macro emits `const SIZE: usize` per registration. Merging changes the total, so `App::build()` recomputes the budget on the merged tree using `max(len(default), len(each localization))` per field. |
| **CMD-06** Unique option names | `name` unique within each options array **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time (within one struct) / App-build (merged) | `derive(Options)` uses field names (or `#[option(rename)]`) and rejects duplicates. Two Modules contributing the same subcommand name under one base fail at App-build (see REG-04). |
| **CMD-07** Age-restricted flag | `nsfw: bool` per command. Apps with discovery enabled cannot contain any age-restricted commands. **[D]** | [AC §Age-Restricted Commands](https://discord.com/developers/docs/interactions/application-commands#age-restricted-commands) | Compile-time (type) / App-build (merge conflicts) | `nsfw` is a `bool` attribute. Conflicting values on one base command fail at App-build (FR-9). The discovery restriction is not checkable locally and is only documented. |

## 2. Command structure: subcommands, groups and Command Patterns

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CMD-10** Nesting depth | Only `command → group → subcommand`. Groups cannot contain groups; subcommands cannot contain subcommands or groups. A Command Path has at most **3** words. **[D]** | [AC §Subcommands and Subcommand Groups](https://discord.com/developers/docs/interactions/application-commands#subcommands-and-subcommand-groups) | Compile-time | The macro expands the Command Pattern and rejects any expansion with more than 3 words. |
| **CMD-11** Name is either a subcommand or a group | One name at one level cannot be both a leaf subcommand and a subcommand group (`config set` and `config set prefix` together are invalid). **[D]** (follows from the option tree: a `SUB_COMMAND` option has no subcommand children) | [AC §Subcommands and Subcommand Groups](https://discord.com/developers/docs/interactions/application-commands#subcommands-and-subcommand-groups) | Compile-time (within one pattern) / App-build (across registrations and Modules) | The macro builds the expansion tree and checks every node. See [Compile-time strategy §3](#3-command-pattern-expansion-and-structural-validation). |
| **CMD-12** Base command with subcommands is not invokable | Once a command has subcommands or groups, the base command itself cannot be used, and it cannot have its own non-subcommand options. **[D]** | [AC §Subcommands and Subcommand Groups](https://discord.com/developers/docs/interactions/application-commands#subcommands-and-subcommand-groups) | Compile-time (one pattern, e.g. `config (set)?`) / App-build (Module A registers `config`, Module B registers `config set`) | A pattern whose expansions contain both `x` and `x y` is a compile error. Across Modules this is a `BuildError` (FR-9: no silent dropping). |
| **CMD-13** Max options / subcommands / groups per level | `options` array max **25** at every level (top-level options, subcommands + groups under a command, subcommands under a group, parameters under a subcommand). **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time (per struct / per pattern) / App-build (after merge) | `derive(Options)` counts fields. The pattern macro counts distinct children per node. The merged tree is recounted in `App::build()`. |
| **CMD-14** Mixing groups and subcommands | A command may hold groups and plain subcommands side by side. **[D]** | [AC §Subcommands and Subcommand Groups](https://discord.com/developers/docs/interactions/application-commands#subcommands-and-subcommand-groups) | (permitted; no check) | Documented so the validator does not reject a valid tree. |
| **CMD-15** Pattern expansion bound | Noctis caps one Command Pattern at **25** expansions by default (FR-8), configurable with `#[command(max_expansions = n)]`, and rejects patterns where an optional group makes two expansions identical. **[N]** | — | Compile-time (literal patterns) / App-build (dynamically built patterns) | Guards against accidental combinatorial patterns. Expansion order is left to right in declaration order (FR-8). |

## 3. Command options

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **OPT-01** Required before optional | Required options must be listed before optional ones. **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | `derive(Options)`: non-`Option<T>` fields are required. A required field after an optional one is a compile error (FR-10); the derive never reorders fields. |
| **OPT-02** Option types | `STRING 3, INTEGER 4, BOOLEAN 5, USER 6, CHANNEL 7, ROLE 8, MENTIONABLE 9, NUMBER 10, ATTACHMENT 11` (1, 2 are subcommand/group) **[D]** | [AC §Option type](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-type) | Compile-time | Field type → option type via a trait (`impl CommandOption for i64`, `Id<UserMarker>`, `Attachment`, …). Unsupported field types do not implement the trait. |
| **OPT-03** INTEGER range | −2^53+1 … 2^53−1 **[D]** | [AC §Option type](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-type) | Compile-time (literal `min_value`/`max_value`, choice values) / Runtime (decoding) | Literal bounds checked in the derive. Decoded values outside `i64` range are a typed error. |
| **OPT-04** NUMBER range | −2^53 … 2^53 (double) **[D]** | [AC §Option type](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-type) | Compile-time (literals) | As OPT-03. |
| **OPT-05** `min_value` / `max_value` | Only on INTEGER / NUMBER; `min ≤ max` **[D]** (ordering implied) | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | `#[option(min = 1, max = 99)]` is accepted only on numeric fields; the derive checks `min ≤ max` and that choice values lie within range. |
| **OPT-06** `min_length` / `max_length` | STRING only; `min_length` 0-6000, `max_length` 1-6000, `min ≤ max` **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | Same derive pass. |
| **OPT-07** `channel_types` | CHANNEL options only **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | Attribute accepted only on channel fields; values are a typed `ChannelType` enum. |
| **OPT-08** Choices count | Max **25** choices per option; STRING / INTEGER / NUMBER only **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | Choices come from a `derive(Choices)` enum (one variant = one choice); the derive counts variants. |
| **OPT-09** Choice name / value length | `name` 1-100 chars; string `value` ≤ 100 chars; value type matches the option type **[D]** | [AC §Choice structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-choice-structure) | Compile-time (literal) / App-build (localized choice names from runtime-loaded locales) | `#[choice(name = "…", value = "…")]` literals measured in the derive; value type comes from `#[repr]` / field type. |
| **OPT-10** Choices exclusive with autocomplete | `autocomplete: true` may not be set when `choices` are present **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | An option is either a `Choices` enum or `#[option(autocomplete = handler)]`, never both; the derive rejects the combination. |
| **OPT-11** Autocomplete types | STRING / INTEGER / NUMBER only **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time | Attribute only accepted on those field types. |
| **OPT-12** Autocomplete input is unvalidated | Autocomplete values are not confined to suggested choices; client-side validation only; partial data may be missing required options **[D]** | [AC §Autocomplete](https://discord.com/developers/docs/interactions/application-commands#autocomplete) | Compile-time (types) / Runtime (decoding) | The autocomplete handler receives a `Partial<Options>` where every field is `Option<T>`; the final handler re-validates. |
| **OPT-13** `file_types` filter | ATTACHMENT only; max **10** entries; each is `image`, `video`, `audio` or a dot-prefixed extension; matches the extension only **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure), [REF §File Type Filtering](https://discord.com/developers/docs/reference#file-type-filtering) | Compile-time | Literal list parsed in the derive; Noctis still checks the real content type at runtime because the filter is extension-only (FR-19). |
| **OPT-14** `required` not on subcommands | `required` is invalid on SUB_COMMAND / SUB_COMMAND_GROUP **[D]** | [AC §Option structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-option-structure) | Compile-time (by construction) | Subcommands are generated from the pattern; the user cannot set `required` on them. |

## 4. Localization

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **LOC-01** Locale keys | Only the locales in Discord's list: `id, da, de, en-GB, en-US, es-ES, es-419, fr, hr, it, lt, hu, nl, no, pl, pt-BR, ro, fi, sv-SE, vi, tr, cs, el, bg, ru, uk, hi, th, zh-CN, ja, zh-TW, ko` (32) **[D]** | [REF §Locales](https://discord.com/developers/docs/reference#locales) | Compile-time | `Locale` is a closed enum generated from this list. Locale files are named by code (`tr.yaml`); the build-time locale codegen (FR-42) rejects unknown file names. |
| **LOC-02** Localized values follow base rules | `name_localizations` follow CMD-01/02/03; `description_localizations` follow CMD-04; choice `name_localizations` follow OPT-09 **[D]** | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (locales embedded at build) / App-build (`LocaleSource` adapter loaded at startup) | The same validators run over every localized value. Word-by-word name translation (FR-41) validates each translated word. |
| **LOC-03** Localized option names distinct | A localized option name must differ from all default option names of that command and from all other option names in the same locale on that command **[D]** | [AC §Localization](https://discord.com/developers/docs/interactions/application-commands#localization) | Compile-time (embedded locales) / App-build | Uniqueness set per command per locale. |
| **LOC-04** Partial localization allowed | Not every locale is required; missing ones fall back to the default **[D]** | [AC §Localization](https://discord.com/developers/docs/interactions/application-commands#localization) | (permitted) | FR-42: a key missing in a non-default Locale is a build warning, not an error. |
| **LOC-05** Localization affects size budget | Longest variant per field counts toward the 8000 budget **[D]** | [AC §Slash Commands](https://discord.com/developers/docs/interactions/application-commands#slash-commands) | see CMD-05 | |
| **LOC-06** Interaction payloads use default names | Dispatch uses default (unlocalized) names; `locale` / `guild_locale` fields give the user's language **[D]** | [AC §Localization](https://discord.com/developers/docs/interactions/application-commands#localization) | Runtime (by construction) | The router matches default names only; User Locale and Guild Locale resolution follows FR-43. |

## 5. Context-menu commands and command metadata

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CTX-01** Context-menu name | 1-32 characters; mixed case and spaces allowed (CMD-01/03 do not apply) **[D]** | [AC §Naming](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-naming) | Compile-time (literal) / App-build (loaded locales) | `#[user_command("Show Profile")]` / `#[message_command(…)]` check length only. |
| **CTX-02** No description, no options | `description` must be the empty string; `options` only for `CHAT_INPUT` **[D]** | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (by construction) | Context-menu macros have no description/options parameters; Noctis sends `""`. |
| **CTX-03** Command types | `CHAT_INPUT 1`, `USER 2`, `MESSAGE 3`, `PRIMARY_ENTRY_POINT 4` **[D]** | [AC §Command types](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-types) | Compile-time | Type comes from the macro used. Primary Entry Point (Activities) is out of scope for v1 (PRD §8.2) and has no macro. |
| **META-01** `default_member_permissions` | Bit set of permissions serialized as a string; `"0"` = only admins (and overwrites) **[D]** | [AC §Using Default Permissions](https://discord.com/developers/docs/interactions/application-commands#application-command-permissions-object-using-default-permissions) | Compile-time (typed bitflags) / App-build (merge conflicts) | `#[command(…, permissions = MANAGE_GUILD \| BAN_MEMBERS)]` parses into `Permissions` bitflags; unknown names are compile errors. Two Modules publishing different values on one base command fail at App-build (FR-9). |
| **META-02** Permissions apply per base command | Only the top-level command carries `default_member_permissions`, `contexts`, `integration_types`, `nsfw`; subcommands cannot differ **[D]** (fields exist only on the command object) | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (one pattern) / App-build (merge) | Setting these on a pattern that targets `x y` sets them on base `x`; a conflicting value for the same base is a `BuildError`. |
| **META-03** `contexts` / `integration_types` only for global commands | Ignored / invalid for guild-scoped commands **[D]** | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (scope given in the attribute) / App-build (scope set on the Module) | Error if a guild-scoped command sets either field. |
| **META-04** `integration_types` must be enabled on the app | Included contexts must already be supported in the app's settings **[D]** | [AC §Installation Context](https://discord.com/developers/docs/interactions/application-commands#installation-context) | Runtime (Publish) | Not knowable offline; Publish reads the application object and reports a typed error before the PUT. |
| **META-05** `dm_permission` deprecated | Use `contexts` **[D]** | [AC §Command structure](https://discord.com/developers/docs/interactions/application-commands#application-command-object-application-command-structure) | Compile-time (by construction) | Not exposed by Noctis. |

## 6. Registration, scope and Publish

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **REG-01** Global command counts | 100 global `CHAT_INPUT`, 15 global `USER`, 15 global `MESSAGE`, 1 `PRIMARY_ENTRY_POINT` **[D]** | [AC §Registering a Command](https://discord.com/developers/docs/interactions/application-commands#registering-a-command) | App-build | Counted after merging all Modules (a merged base command counts once). |
| **REG-02** Guild command counts | Same counts per guild (no Primary Entry Point guild command) **[D]** | [AC §Registering a Command](https://discord.com/developers/docs/interactions/application-commands#registering-a-command) | App-build (statically guild-scoped Modules) / Runtime (Publish with per-guild enable, FR-45) | The per-guild command set can change when a Module is enabled for one guild, so Publish re-counts for that guild before writing. |
| **REG-03** Unique names per type per scope | No two global `CHAT_INPUT` with the same name; same for each type and per guild. Global and guild commands may share a name; different types may share a name. **[D]** | [AC §Registering a Command](https://discord.com/developers/docs/interactions/application-commands#registering-a-command) | App-build | Same-name chat-input registrations are **merged** (FR-9), not rejected; same-name context-menu commands are a `BuildError`. |
| **REG-04** Merge conflicts | Merged registrations must not conflict on metadata (META-01/02), must not create a duplicate leaf path, and must keep CMD-11/12/13/05 after merging **[D]** + **[N]** (FR-9) | AC (as above) | App-build | `App::build()` merges all Modules, then runs the full structural validator on the merged trees. |
| **REG-05** Create quota | 200 application command **creates** per day per guild; bulk overwrite counts new commands toward it **[D]** | [AC §Registering a Command](https://discord.com/developers/docs/interactions/application-commands#registering-a-command) | Runtime (Publish) | Publish diffs local vs remote and writes nothing when equal (FR-13); dry run shows how many creates a PUT would cost. |
| **REG-06** Command permission overwrites | Max 100 per command per guild; needs a Bearer token **[D]** | [AC §Permissions](https://discord.com/developers/docs/interactions/application-commands#permissions) | (out of scope) | Noctis does not edit overwrites in v1. |

## 7. Interactions and responses

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **INT-01** Initial response deadline | Initial response within **3 s**, or the token is invalidated **[D]** | [RR §Interaction Callback](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-callback) | Runtime | Auto-defer deadline per interaction (FR-21): default 2500 ms measured from gateway receipt, configurable; disabled for `opens_modal` handlers (INT-05). |
| **INT-02** Token lifetime | Token valid **15 min** for edits and follow-ups **[D]** | [RR §Followup Messages](https://discord.com/developers/docs/interactions/receiving-and-responding#followup-messages) | Runtime | The Responder's runtime state machine stores the gateway receive time; calls after expiry return `Error::TokenExpired` without an HTTP request. |
| **INT-03** Exactly one initial callback | One callback per interaction; later output goes through edit-original or follow-ups **[D]** | [RR §Responding to an Interaction](https://discord.com/developers/docs/interactions/receiving-and-responding#responding-to-an-interaction) | Compile-time (typestate) / Runtime (auto-defer race) | Typestate: `Responder<Pending>` methods (`reply`, `defer`, `update`, `modal`) consume `self` and return `Responder<Acked>`, which only offers `edit_original`, `followup` and `delete_original`. A runtime state machine, shared by the handler and the auto-defer task, covers what types can't: when the auto-defer wins, the handler's `Pending` call becomes `edit_original`. |
| **INT-04** Callback types per interaction type | `DEFERRED_UPDATE_MESSAGE 6` and `UPDATE_MESSAGE 7` only for component and modal-submit interactions; `MODAL 9` not for `MODAL_SUBMIT` or `PING`; `APPLICATION_COMMAND_AUTOCOMPLETE_RESULT 8` for autocomplete **[D]** | [RR §Callback Type](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-interaction-callback-type) | Compile-time | Each handler kind gets its own context type: `CommandCtx` has no `update()`, `ModalSubmitCtx` has no `modal()`, `AutocompleteCtx` has only `respond(choices)`. |
| **INT-05** Modal must be the first response | A modal is an initial callback (type 9), so it cannot follow a defer or a reply. Modals cannot be chained from a modal submit. **[D]** | [RR §Callback Type](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-interaction-callback-type) | Compile-time (typestate) / Runtime (auto-defer race) | `modal()` exists only on `Responder<Pending>` of command and component contexts whose handler is declared `opens_modal` (for example `#[route("id", opens_modal)]`). The declaration disables auto-defer for that handler, because a defer would make the modal impossible. |
| **INT-06** Autocomplete cannot be deferred | Only callback type 8 is valid; no defer type exists for autocomplete **[D]** (by the callback table) | [RR §Callback Type](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-interaction-callback-type) | Compile-time (no `defer` on `AutocompleteCtx`) / Runtime (3 s timeout returns empty choices) | |
| **INT-07** Autocomplete choices | Max **25** choices per response; choice rules as OPT-09 **[D]** | [RR §Autocomplete](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-autocomplete) | Runtime | `Choices` is a bounded collection (`try_push` fails at 25). Returned choices are always truncated to 25 with a warning (FR-10), in both Strict and Lenient mode. |
| **INT-08** Flags on deferred reply | With `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE (5)` the only valid flag is `EPHEMERAL`; `IS_COMPONENTS_V2` goes on the later edit **[D]** | [RR §Messages](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-messages) | Compile-time (by construction) | `defer()` takes only an `Ephemerality` value; the V2 flag is applied automatically on edit-original (FR-21). |
| **INT-09** Allowed flags on callback messages | Only `SUPPRESS_EMBEDS`, `EPHEMERAL`, `IS_COMPONENTS_V2`, `IS_VOICE_MESSAGE`, `SUPPRESS_NOTIFICATIONS` **[D]** | [RR §Messages](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-messages) | Compile-time | Typed per-endpoint flag sets (see MSG-07). |
| **INT-10** Ephemerality is fixed at creation | A message's ephemeral state cannot change after it is sent **[D]** | [RR §Callback Type](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-interaction-callback-type), research §2.4 | Compile-time (static attribute) | Each Command and Route declares `ephemeral` (or the default) in its attribute; the auto-defer uses it. There is no API to toggle it on edit. |
| **INT-11** Follow-up cap for user-installed apps | Max **5** follow-ups per interaction when the app is user-installed and not installed in the guild (`authorizing_integration_owners` has only `USER_INSTALL`) **[D]** | [RR §Create Followup Message](https://discord.com/developers/docs/interactions/receiving-and-responding#create-followup-message) | Runtime | The Responder's runtime state machine counts follow-ups and returns a typed error on the 6th in that context. |
| **INT-12** Follow-up after defer edits the original | The first follow-up after a type-5 defer acts as edit-original **[D]** | [RR §Create Followup Message](https://discord.com/developers/docs/interactions/receiving-and-responding#create-followup-message) | Runtime (by construction) | Noctis always calls edit-original explicitly after a defer to avoid ambiguity. |
| **INT-13** Interaction endpoints and the global rate limit | Interaction endpoints are not bound by the 50 req/s global limit **[D]** | [RL §Global Rate Limit](https://discord.com/developers/docs/topics/rate-limits#global-rate-limit) | Runtime | Handled by twilight-http-ratelimiting, or by the shared proxy when one is configured (RL-02). |

## 8. `custom_id` and Routes

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CID-01** Length | **1-100** characters on buttons, selects, text inputs, file uploads, radio/checkbox groups, checkboxes and modals **[D]** (counting unit **[U]**) | [CR §Anatomy of a Component](https://discord.com/developers/docs/components/reference#anatomy-of-a-component) | Compile-time | `derive(Route)` computes `MAX_LEN = FMT_LEN + route.len() + SEP_LEN + S + E + N + max(inline, by_ref)` (see [Compile-time strategy §1-2](#1-the-custom_id-layout-and-fixed-length-refs)) and emits a const assertion that it is ≤ 100. An inline Route with an unbounded field does not compile; it must declare `#[route("id", by_ref)]` or `#[route("id", overflow = spill)]`. Because a Route id is at most 24 characters, a by-ref `custom_id` is at most 46 characters. |
| **CID-02** Unique per message | No two components in one message may share a `custom_id` **[D]** | [CR §Anatomy of a Component](https://discord.com/developers/docs/components/reference#anatomy-of-a-component) | Compile-time (static trees with literal Payloads) / Runtime (all other trees) | The `view!` macro rejects two identical (Route, literal Payload) pairs. The runtime validator hashes every `custom_id` in the tree before sending. Disabled "label" buttons (page indicator) still need a unique id. |
| **CID-03** Route format | Route id = `[a-z0-9._-]{1,24}` (so it never contains `:` `,` `~` `!`); ids starting with `_` are reserved for Noctis **[N]** | — | Compile-time | `#[route("shop.buy")]` literal validated in the derive. |
| **CID-04** Route uniqueness | Exactly one handler per Route (FR-15) **[N]** | — | App-build | Routes from all Modules go into one map; duplicates are a `BuildError` naming both Modules. |
| **CID-05** Ref entropy and binding | Ref = 11 base62 characters from the OS CSPRNG (`getrandom`, ≈ 65.5 bits ≥ 64 bits), never sequential, never derived from interaction data; namespaced per App. The stored `RefRecord` carries the Route id and schema version **[N]** (PRD triage) | — | Compile-time (fixed width) / Runtime (generation, record check) | Ref width is a `const`, so every by-ref `custom_id` length is known at compile time. Refs are minted in two phases: `noctis-ui` emits `RefSlot` placeholders and the Responder stores them and finalizes the `custom_id` before sending. A record whose Route differs from the `custom_id`'s Route is treated as forged; a schema mismatch is a Stale Payload. |
| **CID-06** Untrusted input | `custom_id` comes back verbatim from the client and is attacker-controllable **[D]** (returned verbatim) + FR-19 | [CR §Anatomy of a Component](https://discord.com/developers/docs/components/reference#anatomy-of-a-component) | Runtime | Decode never panics; malformed ids, unknown Routes, bad integrity tags and missing Refs produce typed errors / the Expired Handler. |
| **CID-07** ASCII-only encoding | Noctis-encoded Routes, separators, Payload fields and Refs use ASCII only **[N]** | — | Compile-time (by construction) | Makes the CID-01 counting unit irrelevant: 1 char = 1 byte = 1 UTF-16 unit. User strings in Payloads are escaped into ASCII; their budget is the escaped width (see §1 below). |

## 9. Components V2 (messages)

Values from `research/discord-components-v2.md` §0-§2, rechecked against
[CR](https://discord.com/developers/docs/components/reference).

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **CV2-01** V2 flag | `IS_COMPONENTS_V2 = 1 << 15` (32768) on every V2 send/edit; cannot be removed once set **[D]** | [MSG §Message Flags](https://discord.com/developers/docs/resources/message#message-object-message-flags) | Compile-time (by construction) | `V2Message` always serializes the flag; there is no API to clear it. |
| **CV2-02** No `content`, `embeds`, `poll`, `sticker_ids` with V2 | 400 if present **[D]** | [CR](https://discord.com/developers/docs/components/reference), research §2.1 | Compile-time | `V2Message` has no such fields. Switching an existing legacy message to V2 nulls them explicitly. |
| **CV2-03** Total components | ≤ **40** per message, counting nested components at every depth (select options and gallery items do not count) **[D]** | [CR](https://discord.com/developers/docs/components/reference), research §2.2 | Compile-time (static trees) / Runtime (dynamic children) | Each node type has `const COST: usize`; tuples sum their elements' costs; `const { assert!(T::COST <= 40) }` in `view!`. Trees containing `Vec`/iterators are counted by the runtime validator, which reports the path and actual count (FR-24). |
| **CV2-04** Text budget | Total Text Display content ≤ **4000** characters per message **[C][U]** (not in Discord docs; counting unit unknown) | research §1.7 (discord.js / JDA / Discord.Net) | Compile-time (literal texts) / Runtime (dynamic text) | Literals summed in the macro using the stricter of chars vs UTF-16 units. Runtime validator does the same. The limit is configurable. |
| **CV2-05** Placement | Container children: Action Row, Text Display, Section, Media Gallery, Separator, File; no Container in Container; Thumbnail only as Section accessory; Button only in Action Row or as accessory **[D]** | [CR §Component Types](https://discord.com/developers/docs/components/reference#component-object-component-types) | Compile-time | Marker traits: `ContainerChild`, `TopLevel`, `Accessory`, `RowItem`. `Container` does not implement `ContainerChild`, so nesting is a type error (FR-23). |
| **CV2-06** Action Row content | 1-5 Buttons, **or** exactly 1 select; never mixed **[D]** | [CR §Action Row](https://discord.com/developers/docs/components/reference#action-row) | Compile-time (arrays) / Runtime (`Vec`) | `ActionRow::buttons([Button; N])` with `const { assert!(1 <= N && N <= 5) }`; `ActionRow::select(S)` takes exactly one select. |
| **CV2-07** Section | 1-3 Text Displays + exactly one accessory (Button or Thumbnail) **[D]** | [CR §Section](https://discord.com/developers/docs/components/reference#section) | Compile-time | `Section::new(texts: [Text; N], accessory: impl Accessory)` with a const assert on N; no constructor without an accessory. |
| **CV2-08** Button rules | Styles 1-4 need `custom_id`, no `url`/`sku_id`; Link (5) needs `url` ≤ 512, no `custom_id`; Premium (6) needs `sku_id`, no `custom_id`/`label`/`url`/`emoji`; `label` ≤ 80 **[D]** | [CR §Button](https://discord.com/developers/docs/components/reference#button) | Compile-time (types; literal labels) / Runtime (dynamic labels, URLs) | Separate types `RouteButton<R>`, `LinkButton`, `PremiumButton`. |
| **CV2-09** Select menus | `placeholder` ≤ 150; `min_values` 0-25, `max_values` 1-25, `min ≤ max`; String Select `options` ≤ 25; option `label`/`value`/`description` ≤ 100; `default_values` count within `[min, max]`; `disabled` only in messages **[D]** | [CR §String Select](https://discord.com/developers/docs/components/reference#string-select) | Compile-time (literals, `derive(Choices)` enums) / Runtime (dynamic options) | Select options from an enum are counted by the derive; `Vec` options checked at runtime. |
| **CV2-10** Media Gallery | 1-10 items; item `description` ≤ 1024 **[D]** | [CR §Media Gallery](https://discord.com/developers/docs/components/reference#media-gallery) | Compile-time (arrays) / Runtime (`Vec`) | Const generic `N` with a const assert. |
| **CV2-11** Thumbnail | `description` ≤ 1024; images only **[D]** | [CR §Thumbnail](https://discord.com/developers/docs/components/reference#thumbnail) | Compile-time (literal) / Runtime | |
| **CV2-12** File component | `attachment://<filename>` only **[D]** | [CR §File](https://discord.com/developers/docs/components/reference#file) | Compile-time (type) / Runtime (attachment exists) | `File::new(AttachmentRef)`; `AttachmentRef` can only be obtained by adding an attachment (FR-30). |
| **CV2-13** Separator, Container colour | `spacing` 1 or 2; `accent_color` 0x000000-0xFFFFFF **[D]** | [CR §Separator](https://discord.com/developers/docs/components/reference#separator), [CR §Container](https://discord.com/developers/docs/components/reference#container) | Compile-time | `Spacing` enum; `Rgb(u32)` constructor checks range in `const fn`. |
| **CV2-14** Numeric `id` | 32-bit int, unique per message; 0 means unset **[D]** | [CR §Anatomy of a Component](https://discord.com/developers/docs/components/reference#anatomy-of-a-component) | Compile-time (by construction) | Noctis assigns ids deterministically from the builder tree. |
| **CV2-15** Legacy messages | Without V2: up to **5** Action Rows, only types 1-8 **[D]** | research §2.7 | Compile-time | `LegacyMessage` accepts only `[ActionRow; N]` with N ≤ 5. |
| **CV2-16** Unknown component types | Must round-trip as opaque raw JSON **[D]** | [CR §Section](https://discord.com/developers/docs/components/reference#section) | Runtime | Raw escape hatch (FR-31) is still counted against CV2-03. |

## 10. Modals

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **MOD-01** Title | ≤ **45** characters **[D]** | [RR §Modal](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-modal) | Compile-time (literal) / Runtime (dynamic/localized) | `modal!` / `derive(Modal)` measures literal titles. |
| **MOD-02** Top-level components | **1-5**: Label, Text Display (Action Row legacy) **[D]** | [RR §Modal](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-modal) | Compile-time | `derive(Modal)`: one field = one top-level component; the derive counts fields. |
| **MOD-03** Allowed children | No Buttons, Sections, Containers, galleries; no `disabled` anywhere **[D]** | [CR §Label](https://discord.com/developers/docs/components/reference#label) | Compile-time | Modal field types implement `ModalInput`; message components do not. Modal select types have no `disabled` field. |
| **MOD-04** Label | `label` ≤ 45, `description` ≤ 100 **[D]** | [CR §Label](https://discord.com/developers/docs/components/reference#label) | Compile-time (literal) / App-build or Runtime (localized) | |
| **MOD-05** Text Input | `min_length` 0-4000, `max_length` 1-4000, `value` ≤ 4000, `placeholder` ≤ 100 **[D]** | [CR §Text Input](https://discord.com/developers/docs/components/reference#text-input) | Compile-time (attribute literals) / Runtime (prefill value) | |
| **MOD-06** File Upload | `min_values` 0-10, `max_values` 1-10; `file_types` ≤ 10 **[D]** | [CR §File Upload](https://discord.com/developers/docs/components/reference#file-upload) | Compile-time | |
| **MOD-07** Radio / Checkbox groups | Radio options 2-10; Checkbox Group options 1-10, `min_values` 0-10, `max_values` 1-10; Checkbox cannot be `required` **[D]** | [CR §Radio Group](https://discord.com/developers/docs/components/reference#radio-group), [CR §Checkbox Group](https://discord.com/developers/docs/components/reference#checkbox-group) | Compile-time (enum variants) | Option sets from `derive(Choices)`. |
| **MOD-08** Modal `custom_id` | 1-100 **[D]** | [RR §Modal](https://discord.com/developers/docs/interactions/receiving-and-responding#interaction-response-object-modal) | as CID-01 | Modal Routes use the same `derive(Route)`. |
| **MOD-09** Submitted values are untrusted | File uploads are ephemeral CDN URLs; size bounded by `attachment_size_limit` **[D]** | [CR §File Upload](https://discord.com/developers/docs/components/reference#file-upload) | Runtime | Decoded into a typed struct; size/type re-checked. |

## 11. Messages, embeds and allowed mentions

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **MSG-01** `content` (legacy) | ≤ **2000** characters **[D]** (counting unit **[U]**) | [MSG §Create Message](https://discord.com/developers/docs/resources/message#create-message) | Compile-time (literal) / Runtime | |
| **MSG-02** Embeds | ≤ **10** embeds; title 256, description 4096, ≤ 25 fields, field name 256 / value 1024, footer 2048, author name 256; all embed text together ≤ **6000** **[D]** | [MSG §Embed Limits](https://discord.com/developers/docs/resources/message#embed-object-embed-limits) | Compile-time (literal) / Runtime | Legacy only; V2 messages cannot carry embeds (CV2-02). |
| **MSG-03** Allowed mentions exclusivity | `parse` is mutually exclusive with `users` / `roles` for the same type (`parse: ["users"]` + `users: [...]` is invalid) **[D]** | [MSG §Allowed Mentions](https://discord.com/developers/docs/resources/message#allowed-mentions-object) | Compile-time | `Mentions` is an enum per type: `Users::All` or `Users::Only(ids)`, so the invalid combination cannot be built. |
| **MSG-04** Allowed mention ids | `users` ≤ 100, `roles` ≤ 100 **[D]** | [MSG §Allowed Mentions](https://discord.com/developers/docs/resources/message#allowed-mentions-object) | Runtime | Bounded collection. |
| **MSG-05** Mention defaults | Interactions and webhooks parse only user mentions by default; Text Display mentions ping **[D]** | [MSG §Default Settings](https://discord.com/developers/docs/resources/message#allowed-mentions-object-default-settings-for-allowed-mentions) | Compile-time (by construction) | Noctis defaults to `parse: []` for Views; pinging is opt-in. |
| **MSG-06** `nonce`, stickers | `nonce` ≤ 25 chars; `sticker_ids` ≤ 3 **[D]** | [MSG §Create Message](https://discord.com/developers/docs/resources/message#create-message) | Runtime | |
| **MSG-07** Flags per endpoint | Create: `SUPPRESS_EMBEDS, SUPPRESS_NOTIFICATIONS, IS_VOICE_MESSAGE, IS_COMPONENTS_V2`; Edit: `SUPPRESS_EMBEDS` (set/unset), `IS_COMPONENTS_V2` (set only); webhooks/follow-ups per research §2.3 **[D]** | [MSG §Create Message](https://discord.com/developers/docs/resources/message#create-message) | Compile-time | Per-endpoint flag types (`CreateFlags`, `EditFlags`, …). |
| **MSG-08** Edit replaces components | PATCH replaces `components` wholesale; `attachments` must list every attachment to keep **[D]** | [MSG §Edit Message](https://discord.com/developers/docs/resources/message#edit-message) | Runtime (by construction) | Views always re-render the whole tree; FR-30 rebuilds the attachment list. |

## 12. Attachments and uploads

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **ATT-01** File size | Default **20 MiB** per file; may be higher (Nitro / boost); `attachment_size_limit` on the interaction gives the effective limit **[D]** | [REF §Uploading Files](https://discord.com/developers/docs/reference#uploading-files) | Runtime | Checked against `attachment_size_limit` (interactions) or 20 MiB (elsewhere) before upload; the Renderer reports byte size. |
| **ATT-02** Request size | ≤ **25 MiB** per message request **[D]** | [MSG §Create Message](https://discord.com/developers/docs/resources/message#create-message) | Runtime | Sum of all files + JSON. |
| **ATT-03** Files per message | **10** attachments per message **[U]** (widely used by libraries; not stated in the current docs) | — | Runtime | Configurable constant. |
| **ATT-04** `files[n]` naming | Each part named `files[n]` with a filename; `n` matches `attachments[].id` **[D]** | [REF §Uploading Files](https://discord.com/developers/docs/reference#uploading-files) | Runtime (by construction) | Generated by FR-30; authors never write `files[n]`. |
| **ATT-05** `attachment://` references | Must refer to a file in the same request (or retained attachment) **[D]** | [REF §Uploading Files](https://discord.com/developers/docs/reference#uploading-files) | Compile-time (type) / Runtime | `AttachmentRef` only exists after `view.attach(bytes)`; unused attachments are dropped on edit. |
| **ATT-06** Alt text | Attachment `description` ≤ 1024 **[D]** | [MSG §Attachment Object](https://discord.com/developers/docs/resources/message#attachment-object) | Compile-time (literal) / Runtime | |
| **ATT-07** Embed image formats | `attachment://` documented for `.jpg .jpeg .png .webp .gif` in embeds; Media Gallery also accepts video **[D]** | [REF §Uploading Files](https://discord.com/developers/docs/reference#uploading-files) | Compile-time (Renderer output enum) | Renderer formats (FR-33) are all in the allowed set. |

## 13. Webhooks

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **WH-01** `with_components` | `?with_components=true` is required for `components` to be respected on Execute / Edit Webhook Message **[D]** | [WH §Execute Webhook](https://discord.com/developers/docs/resources/webhook#execute-webhook) | Compile-time (by construction) | Noctis' webhook client always sets it when components are present. |
| **WH-02** Non-owned webhooks | Can only send **non-interactive** components; application-owned webhooks can send all **[D]** | [WH §Execute Webhook](https://discord.com/developers/docs/resources/webhook#execute-webhook) | Compile-time | `ForeignWebhook::send` takes a `StaticView` whose node types exclude buttons and selects (`Interactive` marker trait not allowed). |
| **WH-03** V2 + webhook body | With V2, `content`, `embeds`, `files[n]`, `poll` → 400 on execute; on edit they must be set to `null` first **[D]** | [WH §Execute Webhook](https://discord.com/developers/docs/resources/webhook#execute-webhook) | Compile-time | Same `V2Message` type as CV2-02. |
| **WH-04** Webhook name | 1-80 characters **[D]** | [WH §Create Webhook](https://discord.com/developers/docs/resources/webhook#create-webhook) | Compile-time (literal) / Runtime | |
| **WH-05** Forum/media channels | Must pass `thread_id` or `thread_name` **[D]** | [WH §Execute Webhook](https://discord.com/developers/docs/resources/webhook#execute-webhook) | Runtime | Requires the channel type, known only at runtime. |

## 14. HTTP rate limits

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **RL-01** Per-route buckets | Track by `X-RateLimit-Bucket`; obey `Retry-After` on 429 **[D]** | [RL](https://discord.com/developers/docs/topics/rate-limits) | Runtime | twilight-http-ratelimiting (pin ≥ 0.17.1, see twilight research). |
| **RL-02** Global limit | **50** requests/s per bot token, shared across processes **[D]** | [RL §Global Rate Limit](https://discord.com/developers/docs/topics/rate-limits#global-rate-limit) | Startup (cluster config) / Runtime | Multi-process deployments should use a shared HTTP proxy, which replaces the local limiter (FR-52). A Cluster config without one logs a startup warning naming the risk **[N]**. |
| **RL-03** Invalid request limit | **10,000** responses with 401/403/429 per 10 minutes per IP → temporary Cloudflare ban **[D]** | [RL §Invalid Request Limit](https://discord.com/developers/docs/topics/rate-limits#invalid-request-limit-aka-cloudflare-bans) | Runtime | Counter + metric; Noctis' pre-send validation exists partly to keep 4xx counts down. |
| **RL-04** Undocumented per-resource limits | e.g. channel name/topic edits ~2 per 10 min **[C]** | twilight / tescord research | Runtime | Surfaced as 429 with `X-RateLimit-Scope: shared`; not hard-coded. |

## 15. Gateway

| Rule | Limit/value | Source | Enforcement | How |
|---|---|---|---|---|
| **GW-01** Identify concurrency | Shards identify in buckets `rate_limit_key = shard_id % max_concurrency`, one per bucket per **5 s**, buckets in order **[D]** | [GW §Sharding](https://discord.com/developers/docs/events/gateway#sharding) | App-build (fetch `GET /gateway/bot` at startup) / Runtime | The identify queue goes through the `IdentifyQueue` port, with buckets built from `max_concurrency` in `session_start_limit`, never `InMemoryQueue::default()` (twilight research §1.2). A Cluster Shard Range not aligned to `max_concurrency` is accepted and logged as a warning at startup (FR-3). |
| **GW-02** Daily identify budget | **1000** identifies per 24 h (higher for large bots), excluding RESUMEs **[D]** | [GW §Identifying](https://discord.com/developers/docs/events/gateway#identifying) | App-build / Runtime | Startup refuses to boot more shards than `remaining` allows; resumes are preferred (FR-4). |
| **GW-03** Guilds per shard | ≤ **2500**; sharding mandatory beyond that **[D]** | [GW §Sharding](https://discord.com/developers/docs/events/gateway#sharding) | App-build (recommended shard count) | Uses the recommended count unless a fixed count is configured; a too-small fixed count is a `BuildError`. |
| **GW-04** Gateway send rate | **120** events per connection per 60 s **[D]** | [GW §Rate Limiting](https://discord.com/developers/docs/events/gateway#rate-limiting) | Runtime | twilight-gateway's send ratelimiter. |
| **GW-05** Privileged intents | Must be enabled in the Developer Portal and passed explicitly **[D]** | [GW §Privileged Intents](https://discord.com/developers/docs/events/gateway#privileged-intents) | App-build | Required intents are derived from registered Event Handlers (FR-6); `build()` errors when a handler needs an intent the App did not request. Portal state is checked at connect (close code 4014). |
| **GW-06** `large_threshold` | 50-250 **[D]** | [GWE §Identify](https://discord.com/developers/docs/events/gateway-events#identify) | Compile-time (literal) | `LargeThreshold::new::<N>()` with a const assert. |
| **GW-07** Request Guild Members | One `guild_id` per request; `user_ids` up to 100; full list needs `GUILD_MEMBERS`; rate-limited opcode **[D]** | [GWE §Request Guild Members](https://discord.com/developers/docs/events/gateway-events#request-guild-members) | Compile-time (types) / Runtime | `Event::RateLimited` surfaced to the caller. |

---

## Compile-time strategy

The goal is: if the input is written in the source code, a violation must fail
`cargo check`, not a Publish or a send. The building blocks below make that possible.

### 1. The `custom_id` layout and fixed-length Refs

The layout is normative in the architecture spine (AD-5) and quoted here. Every
character is ASCII (CID-07), so the CID-01 counting unit does not matter.

```text
custom_id = F route ":" [S] [E] [N] body [T]

F      1 char    base62 digit = 16 * generation + flags
                 flags: 1 = S present, 2 = E present, 4 = N present, 8 = T present
                 v1 writes generation 0 (F < 16); F >= 16 is a CodecError
route  1..24     [a-z0-9._-], unique per App; ids starting with "_" are reserved
S      1 char    Route schema version 1..=61 (#[route(schema = n)]); absent = 0
E      5 chars   View envelope: action 1, tab 1, page 2, state version 1
N      2 chars   per-render select nonce (FR-25); select Routes only
body   inline    field ("," field)*
       by ref    "~" + 11 base62 Ref
T      6 chars   integrity tag: base62 of truncated keyed BLAKE3 over all
                 preceding chars; inline bodies of #[route(signed)] only
```

| Constant (`noctis-codec`) | Value |
|---|---|
| `FMT_LEN` | 1 |
| `SEP_LEN` | 1 |
| `SCHEMA_LEN` | 1 |
| `VIEW_ENV_LEN` | 5 |
| `NONCE_LEN` | 2 |
| `REF_MARKER_LEN` | 1 |
| `REF_LEN` | 11 |
| `TAG_LEN` | 6 |

A Ref is always exactly **11 base62 characters** (CID-05). 62^11 ≈ 5.2 × 10^19, which
is ≈ 65.5 bits, above the 64-bit entropy floor. Because every part except the inline
body has a fixed width, the worst case is known at compile time:

```text
MAX_LEN = FMT_LEN + route.len() + SEP_LEN + S + E + N + max(inline, by_ref)
inline  = Payload::MAX_ENCODED_LEN + T
by_ref  = REF_MARKER_LEN + REF_LEN = 12
S, E, N, T count only when the Route declares them (1, 5, 2, 6), else 0
```

A Route id is at most 24 characters, so a by-ref `custom_id` is at most
1 + 24 + 1 + 1 + 5 + 2 + 12 = 46 characters: `#[route("wizard.step", by_ref)]` and
`overflow = spill` Routes always fit, and the derive still proves it with a const
assertion. A `signed` Route can't be `by_ref` (the Ref is already unguessable).

Decoding is total. A flag set that doesn't match the Route's declared shape, or a
schema that differs from the Route's, is a Stale Payload (FR-60). An unknown Route or
a bad tag is a forged Payload (FR-19). Appending `Option<T>` fields is compatible and
needs no schema bump; every other Payload change bumps `schema`.

### 2. Bounded Payload fields → computable `custom_id` max length

Every type that can appear in a Payload implements `noctis_codec::Field` with an
associated const for its worst-case encoded width, escaping included:

```rust
pub trait Field {
    /// Worst-case encoded width in ASCII chars, or `None` if unbounded.
    const MAX_LEN: Option<usize>;
}
// Widths (base62, ASCII):
// bool                → Some(1)
// u8, i8              → Some(2)   // 255 = "47"; signed values are zig-zag encoded
// u16, i16            → Some(3)   // 62^3  = 238_328 > 65_535
// u32, i32            → Some(6)   // 62^6  > 4_294_967_295
// u64, i64, Id<T>     → Some(11)  // 62^11 > 2^64 − 1
// enum, ≤ 62 variants → Some(1)   // derive(PayloadEnum)
// Option<T>           → T::MAX_LEN + 1 (presence tag)
// BoundedStr<N>       → Some(2 * N)  // ASCII only; worst case after `!` escaping
// String, Vec<T>      → None
```

`derive(Route)` never inspects type names. It sums `<F as Field>::MAX_LEN` plus
`fields − 1` separators through codec `const fn`s and emits a free (non-generic) const
item, which rustc evaluates during `cargo check`:

```rust
#[derive(Route)]
#[route("shop.buy")]
struct BuyButton { item: Id<ItemMarker>, qty: u8 }

// expands (simplified) to:
impl Route for BuyButton {
    const ID: RouteId = RouteId::new("shop.buy");
    const SHAPE: RouteShape = RouteShape::INLINE;
    const SCHEMA: u8 = 0;
    type Payload = Self;
}
impl Payload for BuyButton {
    const MAX_ENCODED_LEN: Option<usize> = ::noctis::__private::codec::sum_fields(&[
        <Id<ItemMarker> as Field>::MAX_LEN,
        <u8 as Field>::MAX_LEN,
    ]); // 11 + 2 + 1 separator = 14
}
// MAX_LEN = 1 + 8 + 1 + 14 = 24
const _: () = ::noctis::__private::codec::assert_fits::<BuyButton>(); // CID-01
```

The one attribute syntax is `#[route("id")]`, `#[route("id", by_ref)]` or
`#[route("id", overflow = spill)]`, optionally with `schema = n`, `view`, `signed`
and `opens_modal`. If `MAX_ENCODED_LEN` is `None` (an unbounded field) or the bound
exceeds 100 and the Route is not `by_ref` or `overflow = spill`, `assert_fits` panics
in const context with a message naming the rule ("CID-01: route `shop.buy` has
unbounded fields; bound them (`BoundedStr<N>`) or add `by_ref`"). Notes:

- Const panic messages cannot format values, so the message cannot print the computed
  length. A companion `#[test]` generated by the derive prints the full width breakdown
  when run.
- The assertion is a free `const _` on a concrete type, so it fires under `cargo check`.
  Generic Routes are rejected by the derive; a post-monomorphization assertion
  would only fire under `cargo build`.

### 3. Command Pattern expansion and structural validation

`#[command("…")]` (and `command!`) expands the Command Pattern **inside the proc macro**,
using the semantics FR-8 adopts from tescord, written out here:

1. Groups are `(a|b|…)`; a trailing `?` adds the variant with the group removed.
2. Expansion is left to right in declaration order (FR-8); whitespace is collapsed;
   duplicates are rejected, and more than 25 expansions (the default cap,
   `#[command(max_expansions = n)]` changes it) is an error (CMD-15).
3. Each expansion is split into words: 1 word = command, 2 = subcommand,
   3 = group + subcommand; more than 3 is CMD-10.
4. All expansions are inserted into one tree, and the macro checks every node:
   name rules (CMD-01..03, via `noctis-rules`), CMD-11 (a node may not be both a leaf and a group),
   CMD-12 (a base with children may not be a leaf), CMD-13 (≤ 25 children), and the
   size budget CMD-05.

**Example that fails.** The PRD's own example:

```rust
#[command("config (set|get) (prefix|welcome)?")]
async fn config(ctx: CommandCtx, which: ConfigPath) -> Result<()> { … }
```

expands to:

```text
config set prefix   → set = GROUP,      prefix = subcommand
config set welcome  → set = GROUP,      welcome = subcommand
config get prefix   → get = GROUP
config get welcome  → get = GROUP
config set          → set = SUBCOMMAND   ✗ already a GROUP
config get          → get = SUBCOMMAND   ✗ already a GROUP
```

`set` becomes both a subcommand (from `config set`) and a subcommand group (from
`config set prefix`). Discord has no way to express that, so the macro emits:

```text
error: [CMD-11] `set` would be both a subcommand and a subcommand group of `/config`
 --> src/modules/config.rs:1:11
  |
1 | #[command("config (set|get) (prefix|welcome)?")]
  |           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  = note: `config set` makes `set` a subcommand
  = note: `config set prefix` makes `set` a subcommand group
  = help: remove the `?`: "config (set|get) (prefix|welcome)",
          or move the bare form to a separate path such as "config show"
```

The same tree check runs again in `App::build()` on the **merged** trees, because two
patterns that are each valid can collide once Modules are combined (e.g. Module A:
`"config set"`, Module B: `"config set prefix"`). That case is App-build (REG-04).

### 4. Literal string length checks

Wherever a literal is written in an attribute or macro (descriptions, choice names,
labels, placeholders, modal titles, Text Display text), the proc macro measures it.
It counts both Unicode scalar values and UTF-16 code units and compares the larger
against the limit, because the counting unit is not confirmed for several limits
(see [Unconfirmed values](#unconfirmed-values)).

Builders that accept `&'static str` do it with `const fn` plus an inline `const`
block, so the check happens at compile time even outside a macro:

```rust
let label = const { Label::lit("Configure") }; // const fn asserts len <= 80 (CV2-08)
let label = Label::try_from(db_name)?;          // runtime path, same limit
```

A `const fn` assertion only fails the build when it is evaluated in a const context,
so the public API takes `Label` (not `&str`) and the macros always wrap literals in
`const { … }`.

### 5. Static component trees via types and const generics

- **Placement** (CV2-05, MOD-03, WH-02) is encoded with marker traits; an illegal
  child does not implement the parent's child trait, so it is a type error.
- **Arity** uses const generics: `ActionRow::buttons::<N>`, `Section::new([Text; N], _)`,
  `MediaGallery::new([Item; N])`, each with `const { assert!(…) }` (CV2-06/07/10).
- **Budget**: every node type has `const COST: usize` and
  `const TEXT: Option<usize>` (None if dynamic). Tuples and arrays sum them, so a
  fully static `view!` gets `const { assert!(<V as Node>::COST <= 40) }` (CV2-03) and
  the literal text sum against 4000 (CV2-04).
- **Dynamic parts** (`Vec`, iterators, `Paginator` pages) have no static cost. The
  type then reports a static **lower bound**, which is still asserted at compile time,
  and the runtime validator checks the actual tree before sending (FR-24).
- **Static custom_id uniqueness** (CID-02): the `view!` macro sees every
  `(Route, literal Payload)` pair and rejects duplicates.

### 6. trybuild compile-fail tests per rule

Every rule marked Compile-time has at least one fixture:

```text
tests/ui/
  cmd/CMD-01_uppercase_name.rs        + .stderr
  cmd/CMD-11_subcommand_and_group.rs  + .stderr   (the `config (set|get) (prefix|welcome)?` case)
  opt/OPT-10_choices_and_autocomplete.rs + .stderr
  cid/CID-01_unbounded_payload.rs     + .stderr
  cv2/CV2-05_container_in_container.rs + .stderr
  ...
tests/ui-pass/
  boundary values: exactly 32-char name, 100-char description, 25 options,
  25 choices, 5 buttons, 40 components, 100-char custom_id
```

- One `trybuild::TestCases::compile_fail("tests/ui/**/*.rs")` test and one
  `pass("tests/ui-pass/**/*.rs")` test.
- Each `.stderr` must contain the rule ID, so the mapping from error to this catalog is
  tested too.
- `.stderr` output varies across rustc versions, so the UI tests run on the pinned
  MSRV toolchain in CI only.
- App-build rules get unit tests asserting the exact `BuildError` variant; Runtime
  rules get unit and property tests (and Testing kit cases, FR-51) asserting the typed
  error is returned **before** any HTTP request.
- A CI check compares the rule IDs in this file against the IDs found in `tests/`
  and fails when a Compile-time or App-build rule has no test.

---

## Unconfirmed values

| Rule | What is unconfirmed | Noctis interpretation |
|---|---|---|
| CV2-04 | The 4000-character Text Display budget is enforced by the API but not in Discord's docs; the counting unit (code points, UTF-16 units, bytes) is unknown. | Count the larger of code points and UTF-16 units; limit configurable. |
| CID-01 | "1-100 characters": unit not specified. | Noctis encodes ASCII only, so the unit is irrelevant for framework-built ids; user strings are budgeted after escaping. |
| CMD-02, CMD-04, OPT-09, MOD-*, MSG-01 | "characters" for names, descriptions, labels and content: unit not specified. | Count the larger of code points and UTF-16 units. |
| CMD-05 | 8000-character command budget: unit not specified; which "value" properties count beyond choice values is not spelled out. | Count name + description + choice values (string form for numbers) per field, longest localization per field; larger of code points / UTF-16. |
| CMD-01 | The task brief quoted `'` in the name regex; the official regex uses `ʼ` (ʼ), not ASCII `'`. | Follow the official regex; ASCII `'` is rejected. |
| CMD-11 | Discord does not state "a name cannot be both a subcommand and a group" in one sentence; it follows from the option tree (a `SUB_COMMAND` cannot hold subcommands). | Enforced as a hard error. |
| INT-06 | "Autocomplete cannot be deferred" is implied by the callback-type table, not stated. | No defer for autocomplete. |
| ATT-03 | 10 attachments per message is widely used but not in the current docs. | Configurable constant, default 10. |
| RL-04 | Per-resource limits such as channel renames are undocumented. | Not hard-coded; handled as 429s. |
