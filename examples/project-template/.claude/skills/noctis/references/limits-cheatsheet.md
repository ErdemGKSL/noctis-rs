# Limits and rule ids cheatsheet

Every numeric limit Noctis enforces, with its rule id (the id appears in compile
errors, `BuildError`s and runtime errors). Source of truth: the Discord Rules Catalog
(`docs/discord-rules.md` in the noctis-rs repository). **Where:** C = compile time,
B = App build, R = runtime. [U] = counting unit/value unconfirmed by Discord; Noctis
counts the larger of Unicode scalars and UTF-16 units and keeps the value in `Limits`.

## Limits value

```rust
App::from_env()?.limits(Limits { mode: LimitMode::Lenient, ..Limits::default() });
```

`Strict` (default) fails before sending; `Lenient` truncates text with `…` and warns,
but never relaxes structural rules (CV2-03, CV2-05, CV2-06, CID-01, CID-02).
Autocomplete is always truncated to 25. Lowering a limit at runtime re-checks every
tree; raising one never relaxes a compile-time check.

## Commands

| Id | Limit | Where |
| --- | --- | --- |
| CMD-01 | name word regex `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` (`ʼ` U+02BC, not `'`) | C / B |
| CMD-02 | 1–32 chars per word, no empty words | C / B |
| CMD-03 | lowercase | C / B |
| CMD-04 | description 1–100 | C / B |
| CMD-05 | ≤ 8000 chars per command incl. options, choices, longest localization per field [U] | C / B |
| CMD-06 | unique option names per level | C / B |
| CMD-07 | `nsfw` consistent per base | C / B |
| CMD-10 | ≤ 3 words (command → group → subcommand) | C |
| CMD-11 | a name can't be both subcommand and group | C / B |
| CMD-12 | a base with children can't be invoked or have options | C / B |
| CMD-13 | ≤ 25 options / subcommands / groups per level | C / B |
| CMD-14 | groups and subcommands may coexist | — |
| CMD-15 | pattern ≤ 25 expansions (`max_expansions`), no duplicates | C / B |
| REG-01 | 100 global chat-input, 15 user, 15 message commands | B |
| REG-02 | same per guild | B / R |
| REG-03 | unique names per type per scope (chat-input merges) | B |
| REG-04 | merged trees must not conflict | B |
| REG-05 | 200 creates / day / guild (Publish is diff-based) | R |
| CTX-01 | context-menu name 1–32, mixed case/spaces ok | C / B |
| CTX-02 | context menus: no description, no options | C |
| META-01..05 | permissions, contexts, integration types per base; guild commands can't set contexts/integration types | C / B / R |

## Options

| Id | Limit | Where |
| --- | --- | --- |
| OPT-01 | required before optional | C |
| OPT-03 | INTEGER ±(2^53 − 1) | C / R |
| OPT-04 | NUMBER ±2^53 | C |
| OPT-05 | `min ≤ max`, numbers only | C |
| OPT-06 | `min_length` 0–6000, `max_length` 1–6000, strings only | C |
| OPT-07 | `channel_types` on channels only | C |
| OPT-08 | ≤ 25 choices | C |
| OPT-09 | choice name 1–100, string value ≤ 100 | C / B |
| OPT-10 | choices xor autocomplete | C |
| OPT-11 | autocomplete on string/integer/number only | C |
| OPT-12 | autocomplete input is unvalidated (`Partial<T>`) | C / R |
| OPT-13 | `file_types` ≤ 10, attachment only | C |
| INT-07 | ≤ 25 autocomplete choices (always truncated) | R |

## Localization

| Id | Limit | Where |
| --- | --- | --- |
| LOC-01 | 32 Discord locales (file names) | C |
| LOC-02 | localized values follow CMD/OPT rules | C / B |
| LOC-03 | localized option names unique per command per locale | C / B |
| LOC-04 | partial localization allowed (fallback + warning) | — |
| LOC-05 | longest localization counts toward 8000 | C / B |
| LOC-06 | dispatch uses default names | R |

## Interactions

| Id | Limit | Where |
| --- | --- | --- |
| INT-01 | initial response within 3 s (auto-defer at 2.5 s) | R |
| INT-02 | token valid 15 min (`TokenExpired`) | R |
| INT-03 | exactly one initial callback (typestate) | C / R |
| INT-04 | callback types per interaction kind (context types) | C |
| INT-05 | modal must be first response; no modal after modal submit | C / R |
| INT-06 | no defer for autocomplete | C / R |
| INT-08 | deferred command reply: only `EPHEMERAL`; V2 flag on edit | C |
| INT-09 | callback flags: SUPPRESS_EMBEDS, EPHEMERAL, IS_COMPONENTS_V2, IS_VOICE_MESSAGE, SUPPRESS_NOTIFICATIONS | C |
| INT-10 | ephemerality fixed at creation | C |
| INT-11 | ≤ 5 follow-ups for user-installed apps outside the guild | R |
| INT-12 | Noctis always edits the original explicitly after a defer | R |
| INT-13 | interaction endpoints exempt from the global HTTP limit | R |

## `custom_id` and Routes

| Id | Limit | Where |
| --- | --- | --- |
| CID-01 | 1–100 chars; proven per Route at compile time | C |
| CID-02 | unique per message | C / R |
| CID-03 | Route id `[a-z0-9._-]{1,24}`, `_` prefix reserved | C |
| CID-04 | one handler per Route | B |
| CID-05 | Ref = 11 base62 chars, CSPRNG ≥ 64 bits, App-namespaced | C / R |
| CID-06 | `custom_id` is untrusted client input | R |
| CID-07 | ASCII-only encoding | C |

Layout widths: `F` 1, route ≤ 24, `:` 1, schema 1, view envelope 5, select nonce 2,
Ref marker + Ref 12, integrity tag 6. Field widths: `bool` 1, `u8` 2, `u16` 3,
`u32` 6, `u64`/`Id` 11, `PayloadEnum` 1, `Option<T>` +1, `BoundedStr<N>` 2N,
`String`/`Vec` unbounded. Worst by-ref id: 46.

## Components V2 (messages)

| Id | Limit | Where |
| --- | --- | --- |
| CV2-01 | `IS_COMPONENTS_V2` = 32768 on every send/edit; can't be removed | C |
| CV2-02 | no `content`, embeds, poll, stickers with V2 | C |
| CV2-03 | ≤ 40 components total, all depths (options and gallery items don't count) | C / R |
| CV2-04 | ≤ 4000 chars across all Text Displays [U] | C / R |
| CV2-05 | placement (no Container in Container; Thumbnail only as accessory; Button only in row/accessory) | C |
| CV2-06 | Action Row: 1–5 buttons or exactly 1 select | C / R |
| CV2-07 | Section: 1–3 Text Displays + 1 accessory | C / R |
| CV2-08 | button label ≤ 80; link url ≤ 512; premium = sku only | C / R |
| CV2-09 | select placeholder ≤ 150; min 0–25; max 1–25; ≤ 25 options; option label/value/description ≤ 100; defaults within [min,max]; `disabled` messages only | C / R |
| CV2-10 | Media Gallery 1–10 items; description ≤ 1024 | C / R |
| CV2-11 | Thumbnail description ≤ 1024; images only | C / R |
| CV2-12 | File: `attachment://` only | C / R |
| CV2-13 | Separator spacing 1 or 2; accent 0x000000–0xFFFFFF | C |
| CV2-14 | numeric `id` 32-bit, unique (assigned by Noctis) | C |
| CV2-15 | legacy: ≤ 5 Action Rows, types 1–8 | C |
| CV2-16 | unknown component types round-trip as raw JSON | R |
| — | media URL ≤ 2048 [U] | R |

## Modals

| Id | Limit | Where |
| --- | --- | --- |
| MOD-01 | title ≤ 45 | C / R |
| MOD-02 | 1–5 top-level (Label or Text Display) | C |
| MOD-03 | no buttons/sections/containers/galleries; no `disabled` | C |
| MOD-04 | Label ≤ 45, description ≤ 100 | C / B / R |
| MOD-05 | Text Input min 0–4000, max 1–4000, value ≤ 4000, placeholder ≤ 100 | C / R |
| MOD-06 | File Upload min 0–10, max 1–10, `file_types` ≤ 10 | C |
| MOD-07 | Radio 2–10 options; Checkbox Group 1–10, min 0–10, max 1–10; Checkbox not required | C |
| MOD-08 | modal `custom_id` 1–100 | C |
| MOD-09 | submitted values untrusted; upload URLs ephemeral | R |

## Messages, attachments, webhooks

| Id | Limit | Where |
| --- | --- | --- |
| MSG-01 | legacy `content` ≤ 2000 [U] | C / R |
| MSG-02 | embeds (legacy only): ≤ 10; title 256, description 4096, 25 fields, 6000 total | C / R |
| MSG-03 | allowed mentions: `parse` xor explicit ids per type | C |
| MSG-04 | ≤ 100 user ids, ≤ 100 role ids | R |
| MSG-05 | Views default to `parse: []` | C |
| MSG-06 | nonce ≤ 25; ≤ 3 stickers | R |
| MSG-07 | flags per endpoint | C |
| MSG-08 | edits replace components; attachments list rebuilt | R |
| ATT-01 | ≤ 20 MiB per file default; interaction `attachment_size_limit` wins | R |
| ATT-02 | ≤ 25 MiB per request | R |
| ATT-03 | ≤ 10 attachments per message [U] | R |
| ATT-04 | `files[n]` naming (automatic) | R |
| ATT-05 | `attachment://` must reference an attached file | C / R |
| ATT-06 | alt text ≤ 1024 | C / R |
| ATT-07 | image formats png, jpg/jpeg, webp, gif | C |
| WH-01 | `with_components=true` (automatic) | C |
| WH-02 | foreign webhooks: non-interactive only | C |
| WH-03 | V2 webhook body: no content/embeds/files/poll | C |
| WH-04 | webhook name 1–80 | C / R |
| WH-05 | forum/media channels need thread id or name | R |

## Gateway and HTTP

| Id | Limit | Where |
| --- | --- | --- |
| GW-01 | identify buckets by `max_concurrency`, 1 per bucket per 5 s | B / R |
| GW-02 | daily identify budget (1000+) | B / R |
| GW-03 | ≤ 2500 guilds per shard | B |
| GW-04 | 120 gateway sends / 60 s / shard | R |
| GW-05 | privileged intents enabled in the portal and requested | B |
| GW-06 | `large_threshold` 50–250 | C |
| GW-07 | request members: one guild, ≤ 100 user ids | C / R |
| RL-01 | per-route buckets, obey `Retry-After` | R |
| RL-02 | 50 req/s global per token across processes → shared proxy | startup / R |
| RL-03 | 10,000 invalid requests / 10 min → Cloudflare ban | R |
| RL-04 | undocumented per-resource limits (e.g. channel renames) | R |

## Rendering (Noctis defaults, FR-70)

| Limit | Default |
| --- | --- |
| pixels per job / per frame | 4 MP / 1 MP |
| frames / duration | 90 / 6 s |
| fps | GIF ≤ 50, WebP/APNG ≤ 90 |
| size target | ≈ 2 MB (and ≤ Upload Budget) |
| auto-defer deadline | 2500 ms |
| Ref TTL | 24 h |
| ModuleStateStore poll | 5 s |
