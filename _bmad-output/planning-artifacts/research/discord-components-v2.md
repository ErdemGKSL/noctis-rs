# Discord Components V2: Reference & UX Research for vesper

**Status:** research input for `vesper` (Rust Discord bot framework; primary UI = Components V2 + generated images)
**As of:** 2026-10-19. Source snapshot: `discord/discord-api-docs` @ `f7416a2` (2026-10-16).
**Confidence legend:** **[D]** = stated in official Discord docs; **[C]** = community/library-observed behavior (discord.js, JDA, Discord.Net, pycord), not written in Discord's docs; **[R]** = vesper recommendation / design opinion.

Primary sources:

- Components Reference: <https://discord.com/developers/docs/components/reference> ([source mdx](https://github.com/discord/discord-api-docs/blob/main/developers/components/reference.mdx))
- Components Overview: <https://discord.com/developers/docs/components/overview>
- Using Message Components: <https://discord.com/developers/docs/components/using-message-components>
- Using Modal Components: <https://discord.com/developers/docs/components/using-modal-components>
- Receiving & Responding to Interactions: <https://discord.com/developers/docs/interactions/receiving-and-responding>
- Message resource (flags, create, edit): <https://discord.com/developers/docs/resources/message>
- Webhook resource (`with_components`): <https://discord.com/developers/docs/resources/webhook>
- API Reference (Uploading Files, File Type Filtering): <https://discord.com/developers/docs/reference>
- Component Embeds (link previews): <https://discord.com/developers/docs/link-previews/component-embeds>
- Change log: <https://discord.com/developers/docs/change-log>

---

## 0. Cheat sheet (the numbers vesper must enforce)

| Limit | Value | Src |
|---|---|---|
| `IS_COMPONENTS_V2` message flag | `1 << 15` = **32768** | [D] |
| `EPHEMERAL` flag | `1 << 6` = 64 (V2 + ephemeral = **32832**) | [D] |
| Total components per V2 message (nested ones count) | **40** | [D] |
| Top-level components per V2 message | **no separate limit** (was 10 until 2025-04-29); bounded by 40 total | [D] |
| Children per Container | **no separate limit** (was 10); bounded by 40 total | [D] |
| Total Text Display characters per message | **4000** (sum over all Text Displays, incl. inside Sections/Containers) | [C] |
| Legacy (non-V2) message: top-level | 5 Action Rows | [D] |
| Buttons per Action Row | **5** | [D] |
| Selects per Action Row | **1** (and then nothing else in the row) | [D] |
| `custom_id` | **1-100 chars**, unique within message | [D] |
| `id` (numeric) | 32-bit int, unique within message, auto-assigned if omitted/0 | [D] |
| Button `label` | max **80** chars (design guideline: 34 w/ emoji, 38 w/o) | [D] |
| Link button `url` | max **512** chars | [D] |
| Select `options` | max **25** | [D] |
| Select `placeholder` | max **150** chars | [D] |
| Select `min_values` / `max_values` | 0-25 / 1-25 (defaults 1/1) | [D] |
| Select option `label`/`value`/`description` | max **100** each | [D] |
| Section `components` | **1-3** Text Displays | [D] |
| Section `accessory` | exactly 1: Button or Thumbnail | [D] |
| Thumbnail / gallery item `description` (alt text) | max **1024** chars | [D] |
| Media Gallery `items` | **1-10** | [D] |
| Separator `spacing` | `1` small (default) / `2` large | [D] |
| Container `accent_color` | `0x000000`-`0xFFFFFF` or null | [D] |
| Modal `title` | max **45** chars | [D] |
| Modal `custom_id` | 1-100 chars | [D] |
| Modal top-level components | **1-5** | [D] |
| Label `label` / `description` | max **45** / max **100** | [D] |
| Text Input `min_length` / `max_length` | 0-4000 / 1-4000 | [D] |
| Text Input `value` / `placeholder` | max 4000 / max 100 | [D] |
| File Upload `min_values`/`max_values` | 0-10 / 1-10 (defaults 1/1) | [D] |
| File Upload `file_types` | max **10** entries | [D] |
| Radio Group options | **2-10** | [D] |
| Checkbox Group options | **1-10**, min/max_values 0-10 / 1-10 | [D] |
| Default file upload size | **20 MiB** per file (since 2026-09-03) | [D] |
| Interaction initial response deadline / token life | **3 s** / **15 min** | [D] |
| `content` (legacy) | 2000 chars; **forbidden** with V2 flag | [D] |

---

## 1. Component catalogue

### 1.1 Type ids and placement

Source: [Component Types table](https://discord.com/developers/docs/components/reference#component-object-component-types).

| Type | Name | Category | Message: top-level | In Container | In Section | In Action Row | Modal | Modal parent |
|---|---|---|---|---|---|---|---|---|
| 1 | Action Row | Layout | yes | yes | – | – | deprecated (legacy Text Input wrapper) | top-level |
| 2 | Button | Interactive | no | no (via Action Row / Section) | **accessory** only | yes (≤5) | **no** | – |
| 3 | String Select | Interactive | no | no (via Action Row) | – | yes (alone) | yes | Label |
| 4 | Text Input | Interactive | no | – | – | – | yes | Label (Action Row deprecated) |
| 5 | User Select | Interactive | no | via Action Row | – | yes (alone) | yes | Label |
| 6 | Role Select | Interactive | no | via Action Row | – | yes (alone) | yes | Label |
| 7 | Mentionable Select | Interactive | no | via Action Row | – | yes (alone) | yes | Label |
| 8 | Channel Select | Interactive | no | via Action Row | – | yes (alone) | yes | Label |
| 9 | Section | Layout | yes | yes | – | – | no | – |
| 10 | Text Display | Content | yes | yes | yes (1-3 children) | – | yes | **top-level** |
| 11 | Thumbnail | Content | no | no | **accessory** only | – | no | – |
| 12 | Media Gallery | Content | yes | yes | – | – | no | – |
| 13 | File | Content | yes | yes | – | – | no | – |
| 14 | Separator | Layout | yes | yes | – | – | no | – |
| 17 | Container | Layout | yes | **no** (no nesting) | – | – | no | – |
| 18 | Label | Layout | no | – | – | – | yes | top-level |
| 19 | File Upload | Interactive | no | – | – | – | yes | Label |
| 21 | Radio Group | Interactive | no | – | – | – | yes | Label |
| 22 | Checkbox Group | Interactive | no | – | – | – | yes | Label |
| 23 | Checkbox | Interactive | no | – | – | – | yes | Label |

Notes:

- Types 15, 16 and 20 are unassigned in public docs. **[D]** vesper must treat unknown types as opaque (deserialize to `Unknown { raw }`), since Discord says "Don't hardcode `components` to contain only text components... `accessory` may be expanded" ([Section](https://discord.com/developers/docs/components/reference#section)).
- Container child list (official): Action Row, Text Display, Section, Media Gallery, Separator, File. No Container-in-Container. **[D]**
- Component Embeds (link previews, 2026-10-05) reuse the same model but are display-only, max 40 components incl. the root Container, link buttons only. **[D]** ([Component Embeds](https://discord.com/developers/docs/link-previews/component-embeds))

### 1.2 Common fields

| Field | Type | Rules |
|---|---|---|
| `type` | int | Required. |
| `id?` | int | 32-bit; unique within the message; auto-generated sequentially if omitted; `0` treated as empty and replaced; auto-generation skips ids you set explicitly. Legacy (pre-V2) messages report `id: 0`. **[D]** |
| `custom_id` | string | Interactive components only. **1-100 chars**, must be unique per message. **[D]** |

### 1.3 Action Row (type 1)

| Field | Type | Limits |
|---|---|---|
| `components` | array | **1-5 Buttons** OR **exactly 1 Select** (types 3,5,6,7,8). Never mix. |

Text Inputs in Action Rows in modals are **deprecated**; use Label. **[D]**

### 1.4 Button (type 2)

| Field | Type | Limits / rules |
|---|---|---|
| `style` | int | 1 Primary, 2 Secondary, 3 Success, 4 Danger, 5 Link, 6 Premium |
| `label?` | string | max **80** chars |
| `emoji?` | partial emoji | `id`, `name`, `animated` |
| `custom_id?` | string | 1-100; **required** for styles 1-4; forbidden for 5, 6 |
| `sku_id?` | snowflake | **required** for style 6 only |
| `url?` | string | max **512**; **required** for style 5 only |
| `disabled?` | bool | default `false` |

Style rules **[D]**: non-link/non-premium must have `custom_id` and no `url`/`sku_id`; link must have `url`, no `custom_id`, sends no interaction; premium must have `sku_id` and no `custom_id`/`label`/`url`/`emoji`, sends no interaction. Placement: Action Row or Section `accessory`. Not allowed in modals.
Design guideline **[D]**: label 34 chars max with emoji, 38 without; one Primary per group; equal-weight actions all Secondary.

### 1.5 Select menus (types 3, 5, 6, 7, 8)

Shared fields:

| Field | Applies to | Limits |
|---|---|---|
| `custom_id` | all | 1-100 |
| `placeholder?` | all | max **150** |
| `min_values?` | all | default 1; min 0, max 25. Must be omitted or ≥1 if `required` is omitted/true |
| `max_values?` | all | default 1; max 25 |
| `required?` | all | **modal only**, default `true`; ignored in messages |
| `disabled?` | all | **message only**, default `false`; **using it in a modal is an error** |
| `options` | String (3) | **max 25** select options |
| `default_values?` | 5,6,7,8 | array of `{id, type: "user"\|"role"\|"channel"}`; count within `[min_values, max_values]` |
| `channel_types?` | Channel (8) | array of channel type ints |

Select option (String Select):

| Field | Limits |
|---|---|
| `label` | max 100 |
| `value` | max 100 |
| `description?` | max 100 |
| `emoji?` | partial emoji |
| `default?` | bool |

### 1.6 Section (type 9) **[D]**

| Field | Limits |
|---|---|
| `components` | **1-3** children; currently only Text Display |
| `accessory` | exactly one: Button (any style) or Thumbnail |

Messages only. Section with Thumbnail = 3 components minimum (section + text + thumbnail).

### 1.7 Text Display (type 10)

| Field | Limits |
|---|---|
| `content` | Markdown (headers `#`/`##`/`###`, `-#` subtext, lists, code blocks, mentions, emoji, timestamps). No per-component limit documented. **Message-wide total 4000 chars** across all Text Displays. **[C]** |

Mentions in Text Display **ping**, governed by `allowed_mentions`. **[D]** On edit, `mentions`/`mention_roles` are rebuilt from the components' text. **[D]** Allowed in modals as a top-level component (since 2025-09-10), counting toward the 5. In modal submits it returns `{type:10,id}` only.

> The 4000-char total is enforced by the API but is not written in the docs repo; it's documented by [discord.js guide](https://discordjs.guide/popular-topics/display-components), [JDA TextDisplay](https://docs.jda.wiki/net/dv8tion/jda/api/components/textdisplay/TextDisplay.html), [Discord.Net CV2 guide](https://docs.discordnet.dev/guides/components_v2/advanced.html). Treat as authoritative-in-practice, keep the limit configurable.

### 1.8 Thumbnail (type 11) **[D]**

| Field | Limits |
|---|---|
| `media` | Unfurled Media Item (`url`: https URL or `attachment://<filename>`) |
| `description?` | alt text, max **1024** |
| `spoiler?` | default `false` |

Section accessory only. Images only (incl. animated GIF/WebP); **no video**.

### 1.9 Media Gallery (type 12) **[D]**

| Field | Limits |
|---|---|
| `items` | **1-10** Media Gallery Items |
| item.`media` | Unfurled Media Item (URL or `attachment://`) |
| item.`description?` | max **1024** |
| item.`spoiler?` | default `false` |

Images and video supported. One gallery = **1 component** regardless of item count (items are not components), which makes it the cheapest way to show visual data.

### 1.10 File (type 13) **[D]**

| Field | Limits |
|---|---|
| `file` | Unfurled Media Item, **only `attachment://<filename>`** |
| `spoiler?` | bool |
| `name?`, `size?` | response-only, ignored on send |

One attached file per File component.

### 1.11 Separator (type 14) **[D]**

| Field | Default | Values |
|---|---|---|
| `divider?` | `true` | show a line |
| `spacing?` | `1` | `1` small, `2` large |

### 1.12 Container (type 17) **[D]**

| Field | Limits |
|---|---|
| `components` | Action Row, Text Display, Section, Media Gallery, Separator, File; no count limit beyond 40 total |
| `accent_color?` | int RGB `0x000000`-`0xFFFFFF` or `null` |
| `spoiler?` | default `false` |

### 1.13 Unfurled Media Item **[D]**

Only `url` is settable; `proxy_url`, `height`, `width`, `placeholder` (thumbhash), `placeholder_version`, `content_type`, `flags` (`IS_ANIMATED = 1<<0`), `attachment_id` are filled by Discord. `url` supports arbitrary URLs and `attachment://<filename>`. (Component Embeds additionally document http/https only, max 2048 chars; reasonable defaults for vesper's validator **[R]**.)

### 1.14 Modal-only components

#### Label (type 18) **[D]**

| Field | Limits |
|---|---|
| `label` | max **45** |
| `description?` | max **100** (may render above or below the child depending on platform) |
| `component` | one of: Text Input, String/User/Role/Mentionable/Channel Select, File Upload, Radio Group, Checkbox Group, Checkbox |

Label + child = **2 components**.

#### Text Input (type 4) **[D]**

| Field | Limits |
|---|---|
| `custom_id` | 1-100 |
| `style` | 1 Short, 2 Paragraph |
| `min_length?` | 0-4000 |
| `max_length?` | 1-4000 |
| `required?` | default `true` |
| `value?` | prefill, max 4000 |
| `placeholder?` | max 100 |
| `label` | **deprecated**; not allowed when wrapped in Label |

#### File Upload (type 19) **[D]**

| Field | Limits |
|---|---|
| `custom_id` | 1-100 |
| `min_values?` | default 1, 0-10 |
| `max_values?` | default 1, max 10 |
| `required?` | default `true` |
| `file_types?` | max 10; `image`, `video`, `audio` groups or dot-extension (`.pdf`); extension-only check (since 2026-08-05) |

Max file size = user's upload limit in that channel (`attachment_size_limit` on the interaction).

#### Radio Group (type 21) **[D]** (2026-02-12)

`custom_id` (1-100), `options` **2-10** (`value`≤100, `label`≤100, `description?`≤100, `default?`), `required?` (default true). Submit: `value: string | null`.

#### Checkbox Group (type 22) **[D]**

`custom_id`, `options` **1-10** (same option shape), `min_values?` 0-10 (default 1), `max_values?` 1-10 (default = number of options), `required?` default true. Submit: `values: string[]` (may be `[]`).

#### Checkbox (type 23) **[D]**

`custom_id`, `default?` bool. Cannot be `required` (use a 1-option Checkbox Group instead). Submit: `value: bool`.

---

## 2. Message-level rules for `IS_COMPONENTS_V2`

### 2.1 What the flag changes **[D]**

| Aspect | Behavior with flag `32768` |
|---|---|
| `content` | Not allowed → 400 |
| `embeds` | Not allowed → 400 |
| `sticker_ids`, `poll`, `shared_client_theme` | Not allowed → 400 |
| Webhook `files[n]` + V2 | Webhook docs: providing `content`, `embeds`, `files[n]` or `poll` with V2 fails (see 2.5). Use attachments referenced from components. |
| Attachments | **Not shown by default**; must be surfaced via Media Gallery, Thumbnail or File |
| Total components | **40** including nested |
| Top-level | no separate cap |
| Removing the flag later | **Impossible.** Once set it can't be removed |
| Adding the flag on edit | Allowed: set `IS_COMPONENTS_V2` and clear `content`/`poll` → `null`, `embeds`/`sticker_ids` → `[]` (else 400) |

### 2.2 Counting components (budget math)

Every object with a `type` counts, at every depth. Select options, gallery items and emoji do not. Examples:

| Construct | Cost |
|---|---|
| Container | 1 |
| Text Display | 1 |
| Separator | 1 |
| Media Gallery (1-10 items) | 1 |
| Section + 1 text + button accessory | 3 |
| Section + 3 texts + thumbnail | 5 |
| Action Row + 5 buttons | 6 |
| Action Row + 1 select (25 options) | 2 |

Example dashboard: Container(1) + header Text(1) + Separator(1) + nav Row of 5 buttons(6) + Gallery(1) + 5 Sections each with text+button(15) + Separator(1) + pager Row of 4 buttons(5) = **31 / 40**.

### 2.3 Flags allowed per endpoint **[D]**

| Endpoint | Settable flags |
|---|---|
| Create Message | `SUPPRESS_EMBEDS`, `SUPPRESS_NOTIFICATIONS`, `IS_VOICE_MESSAGE`, `IS_COMPONENTS_V2` |
| Edit Message | `SUPPRESS_EMBEDS` (set/unset), `IS_COMPONENTS_V2` (set only) |
| Interaction callback data (types 4, 7) | `SUPPRESS_EMBEDS`, `EPHEMERAL`, `IS_COMPONENTS_V2`, `IS_VOICE_MESSAGE`, `SUPPRESS_NOTIFICATIONS` |
| `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` (5) | **only `EPHEMERAL`**; set `IS_COMPONENTS_V2` in the later *Edit Original Interaction Response* |
| Execute Webhook | `SUPPRESS_EMBEDS`, `SUPPRESS_NOTIFICATIONS`, `IS_COMPONENTS_V2` |
| Edit Webhook Message | `SUPPRESS_EMBEDS`, `IS_COMPONENTS_V2` |
| Followup message | as Execute Webhook + `EPHEMERAL` (64) |

### 2.4 Interaction callback types relevant to V2 **[D]**

| Name | Value | Use in vesper |
|---|---|---|
| `CHANNEL_MESSAGE_WITH_SOURCE` | 4 | Send a new dashboard (with `flags: 32768` or `32832` for ephemeral) |
| `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` | 5 | Slow render (image gen); only `EPHEMERAL` here, add V2 flag on edit-original |
| `DEFERRED_UPDATE_MESSAGE` | 6 | Component clicked, slow update; no loading state shown |
| `UPDATE_MESSAGE` | 7 | Component clicked → rewrite the dashboard in place (tab switch, pagination) |
| `MODAL` | 9 | Open a modal (not from MODAL_SUBMIT or PING) |
| `LAUNCH_ACTIVITY` | 12 | n/a |

Types 6/7 valid only for component and `MODAL_SUBMIT` interactions. Respond within **3 s**; token valid **15 min** for edits and followups. User-installed apps not installed in the guild: max **5 followups** per interaction.

Ephemeral state of an existing message can't change; the ephemeral flag is fixed at creation.

### 2.5 Webhooks **[D]**

- Add `?with_components=true` to Execute/Edit Webhook, otherwise `components` is **ignored**.
- Application-owned webhooks: all components. Non-owned webhooks: **non-interactive** only (with the query param).
- With V2: Execute Webhook rejects `content`, `embeds`, `files[n]`, `poll`; Edit Webhook Message requires those be `null` first.

### 2.6 Editing & attachments **[D]** + **[C]**

- PATCH replaces `components` wholesale. No partial component patching. Use `id` to *locate* components in received messages, not to diff-update.
- `attachments` on edit must list **all** attachments to keep plus new ones (API v10). Omitted ones are removed. New files are sent as `files[n]` with matching `attachments[].id = n`. **[D]**
- Practical **[C]**: to swap a generated chart, upload `files[0]` named e.g. `chart-<hash>.png`, set `attachments: [{id:0, filename:"chart-<hash>.png"}]` (dropping the old one), and reference `attachment://chart-<hash>.png`. Changing the filename per render avoids client cache confusion.
- Only `.jpg`, `.jpeg`, `.png`, `.webp`, `.gif` documented for `attachment://` in embeds; Media Gallery also supports video. Default upload limit **20 MiB** per file.

### 2.7 Legacy behavior **[D]**

Without the flag: `content`/`embeds` + up to 5 Action Rows; only types 1-8 usable. Legacy is **not deprecated**.

---

## 3. Modals

### 3.1 Structure **[D]**

```jsonc
{
  "type": 9,                       // MODAL
  "data": {
    "custom_id": "setup:v1:step2", // 1-100
    "title": "Server setup (2/4)", // max 45
    "components": [                // 1-5 top-level: Label or Text Display (Action Row legacy)
      { "type": 10, "content": "Pick the log channel and moderators." },
      { "type": 18, "label": "Log channel", "description": "Where moderation logs go",
        "component": { "type": 8, "custom_id": "log_ch", "channel_types": [0], "max_values": 1 } },
      { "type": 18, "label": "Moderator roles",
        "component": { "type": 6, "custom_id": "mod_roles", "min_values": 1, "max_values": 5 } },
      { "type": 18, "label": "Severity",
        "component": { "type": 21, "custom_id": "sev", "options": [
          {"value":"low","label":"Low"}, {"value":"high","label":"High","default":true}] } },
      { "type": 18, "label": "Evidence", "description": "Screenshots (optional)",
        "component": { "type": 19, "custom_id": "files", "min_values": 0, "max_values": 5,
                       "required": false, "file_types": ["image"] } }
    ]
  }
}
```

Rules:

- **1-5 top-level components**. Top-level = Label (preferred), Text Display, Action Row (legacy Text Input wrapper, deprecated).
- **No `disabled`** anywhere in modals (error). No Buttons, Sections, Containers, galleries.
- Modal can't be returned in response to `MODAL_SUBMIT` (no modal chaining directly). Multi-step wizards must go modal → message (button "Continue") → modal.
- `required` defaults to `true` for selects, Text Input, File Upload, Radio Group, Checkbox Group.

### 3.2 Modal submit payload **[D]**

```jsonc
{
  "type": 5,  // MODAL_SUBMIT
  "message": { /* present if the modal was opened from a component */ },
  "data": {
    "custom_id": "setup:v1:step2",
    "components": [
      { "type": 10, "id": 1 },
      { "type": 18, "id": 2, "component": { "type": 8, "id": 3, "custom_id": "log_ch", "values": ["123"] } },
      { "type": 18, "id": 4, "component": { "type": 6, "id": 5, "custom_id": "mod_roles", "values": ["456","789"] } },
      { "type": 18, "id": 6, "component": { "type": 21, "id": 7, "custom_id": "sev", "value": "high" } },
      { "type": 18, "id": 8, "component": { "type": 19, "id": 9, "custom_id": "files", "values": ["999"] } }
    ],
    "resolved": {
      "channels": { "123": { /* partial channel */ } },
      "roles": { "456": {}, "789": {} },
      "attachments": { "999": { "filename": "bug.png", "content_type": "image/png", "ephemeral": true,
                                "url": "https://cdn.discordapp.com/ephemeral-attachments/...", "size": 241394 } }
    }
  }
}
```

Per-child response shapes:

| Child | Key field |
|---|---|
| Text Input (4) | `value: string` |
| String Select (3) | `values: string[]` |
| User/Role/Mentionable/Channel Select (5-8) | `values: snowflake[]` + top-level `data.resolved` |
| File Upload (19) | `values: snowflake[]` → `resolved.attachments` (ephemeral CDN URLs; download promptly) |
| Radio Group (21) | `value: string \| null` |
| Checkbox Group (22) | `values: string[]` |
| Checkbox (23) | `value: bool` |
| Text Display (10) | `{type, id}` only |

In modal submits the child carries `type`; in message component interactions the data carries `component_type`. **[D]**

---

## 4. `id` vs `custom_id`

| | `id` | `custom_id` |
|---|---|---|
| Type | 32-bit int | string 1-100 |
| On | every component | interactive only |
| Uniqueness | per message | per message |
| If omitted | auto-assigned sequentially, skipping explicitly used ids; `0` = omitted | required (except link/premium buttons) |
| Returned in | message object; message-component interaction `data.id`; modal submit children | interaction `data.custom_id` |
| Purpose | stable handle to locate/address a component in a received message (e.g. find the Text Display to read state, or a Label in a submit) | routing + state |

**[R]** vesper should assign `id`s deterministically from the builder tree (e.g., the developer's named slot → stable int) so tests and handlers can find components by id. Don't put routing data in `id`.

---

## 5. Component interaction payloads (messages)

`type: 3` (MESSAGE_COMPONENT). `data`:

| Field | Notes |
|---|---|
| `custom_id` | string |
| `component_type` | int |
| `id` | numeric id of the clicked component (shown in examples) |
| `values?` | always present for selects: option `value`s (string) or snowflakes |
| `resolved?` | users/members/roles/channels for auto-populated selects |

```jsonc
// Button
{ "type": 3, "data": { "component_type": 2, "id": 7, "custom_id": "dash:tab:2" },
  "message": { "flags": 32768, "components": [ /* full current tree */ ] }, "token": "..." }

// Mentionable select
{ "type": 3, "data": { "component_type": 7, "id": 12, "custom_id": "who",
  "values": ["111","222"],
  "resolved": { "users": {"111": {}}, "members": {"111": {}}, "roles": {"222": {}} } } }
```

`interaction.message` contains the **full current component tree**, which vesper can use as a state carrier (e.g. read the hidden page number from the selected/disabled button). Partial members lack `user`, `deaf`, `mute`; partial channels include `app_permissions` (since 2026-07-16). **[D]**

---

## 6. UX patterns for dashboard-quality UIs

All **[R]** unless marked; grounded in the limits above and Discord's design guidance.

### 6.1 Canonical dashboard layout (one Container)

```
Container (accent = brand/status color)
 ├ Section: "## Server Dashboard" + "-# updated <t:..:R>" | accessory: Thumbnail (guild icon)
 ├ ActionRow: [Overview*] [Members] [Logs] [Settings] [⟳]     ← nav bar, current tab disabled
 ├ Separator (divider, spacing 1)
 ├ MediaGallery: attachment://chart-<hash>.png                ← generated image for read-only data
 ├ Section: "**Auto-mod** · enabled" | Button "Configure"
 ├ Section: "**Welcome** · disabled" | Button "Enable" (Success)
 ├ Separator (spacing 2)
 └ ActionRow: [◀] [Page 2/5 (disabled)] [▶]
```

JSON skeleton:

```jsonc
{
  "flags": 32768,
  "components": [{
    "type": 17, "accent_color": 5793266,
    "components": [
      { "type": 9, "components": [
          { "type": 10, "content": "## Server Dashboard\n-# updated <t:1792400000:R>" } ],
        "accessory": { "type": 11, "media": { "url": "attachment://icon.png" }, "description": "Server icon" } },
      { "type": 1, "components": [
          { "type": 2, "style": 1, "label": "Overview", "custom_id": "v1|dash|tab|0", "disabled": true },
          { "type": 2, "style": 2, "label": "Members",  "custom_id": "v1|dash|tab|1" },
          { "type": 2, "style": 2, "label": "Logs",     "custom_id": "v1|dash|tab|2" },
          { "type": 2, "style": 2, "label": "Settings", "custom_id": "v1|dash|tab|3" },
          { "type": 2, "style": 2, "emoji": { "name": "🔄" }, "custom_id": "v1|dash|refresh" } ] },
      { "type": 14, "divider": true, "spacing": 1 },
      { "type": 12, "items": [ { "media": { "url": "attachment://chart-9f2c.png" },
                                 "description": "Messages per day, last 14 days" } ] },
      { "type": 9, "components": [ { "type": 10, "content": "**Auto-mod** · enabled" } ],
        "accessory": { "type": 2, "style": 2, "label": "Configure", "custom_id": "v1|automod|open" } },
      { "type": 14, "spacing": 2 },
      { "type": 1, "components": [
          { "type": 2, "style": 2, "emoji": { "name": "◀" }, "custom_id": "v1|dash|pg|0|1" },
          { "type": 2, "style": 2, "label": "2 / 5", "custom_id": "v1|noop|1", "disabled": true },
          { "type": 2, "style": 2, "emoji": { "name": "▶" }, "custom_id": "v1|dash|pg|0|3" } ] }
    ]
  }]
}
```

Cost: 1+3+6+1+1+3+1+4 = **20 / 40**.

### 6.2 Navigation patterns

| Pattern | When | Cost | Notes |
|---|---|---|---|
| **Button tab bar** (≤5 tabs) | Few top-level views | 1 row + N | Current tab `disabled: true` + Primary style; others Secondary. Respond with `UPDATE_MESSAGE` (7). |
| **String Select nav** | 6-25 destinations, or hierarchical | 2 | Mark current option `default: true`; placeholder "Jump to…". Great on mobile (native picker). |
| **Two-level nav** | Sections + subpages | row of buttons + select | Keep the button row stable across views so muscle memory works. |
| **Breadcrumb in header** | Deep drill-down | 0 extra (text) | `-# Settings › Moderation › Auto-mod` subtext + "Back" button. |

Because both custom_ids must be unique per message, disabled "label" buttons (e.g. page indicator) still need a unique `custom_id`.

### 6.3 Pagination when content exceeds 40 components / 4000 chars

- Paginate by **budget**, not fixed item count: compute per-item cost (component count + text length) and fill pages greedily until either budget is hit (reserve chrome cost: header, nav, pager).
- Pager row: `⏮ ◀ [n/N disabled] ▶ ⏭` (6 components). Disable ◀ on page 1, ▶ on last.
- For >5 pages add a "Go to page" button that opens a modal with a Text Input (or a String Select with up to 25 page choices).
- Prefer rendering long tables as an **image** (one Media Gallery = 1 component, 0 text chars) and keep interactive rows for actionable items only.
- Lists of selectable entities: a String Select (25 options = 2 components) beats 25 Sections (≥75 components).

### 6.4 Setup wizards / step flows

- Header Text: `### Step 2 of 4 · Channels` + a text progress bar (`▰▰▱▱`) or a generated progress image.
- Accent color per state: neutral (in progress), green (complete), red (error). Accent changes are cheap visual feedback.
- Footer row: `[Back] [Skip] [Next (Primary)]`; disable Next until required fields valid (state lives in the store; re-render on each select).
- Collect free text via modal opened from a button; modals can't chain, so after `MODAL_SUBMIT` respond `UPDATE_MESSAGE` with the next step (submit of a modal opened from a message component may update that message).
- Selects in messages fire immediately per change; selects in modals batch. Use modals when 2+ related inputs must be atomic.

### 6.5 Sections & accessories

- Section = "row with an action" (settings lists: label/value + Configure button) or "card with image" (thumbnail).
- Only one accessory; for 2+ actions per item use a following Action Row (cost +N+1) or a select.
- Thumbnail is small; use Media Gallery for anything that must be legible (charts, cards).

### 6.6 Generated images

- Use `attachment://` with unique filenames per render; Media Gallery for full-width, Thumbnail for icons/avatars.
- Always set `description` (alt text, ≤1024) — accessibility and fallback.
- Gallery layout varies with item count (1 = full width; 2-4 = grid; up to 10). For dashboards prefer 1 wide image (e.g. 1600×900 or 2:1) or 2 side-by-side.
- Upload limit 20 MiB default; aim for <1 MB WebP/PNG for latency within the 3 s ACK (defer with type 5/6 for slow rendering).

### 6.7 Text budgeting

- Use markdown headings (`##`, `###`) and `-#` subtext for hierarchy, not extra components.
- Merge adjacent static text into one Text Display (saves components; chars unchanged).
- Use relative timestamps `<t:unix:R>` (short, localized, live-updating).
- Escape user-provided text (markdown, mentions) and set `allowed_mentions: {parse: []}` for dashboards, since Text Display mentions ping. **[D]**

### 6.8 Mobile caveats **[C]**/**[R]**

- Action Rows wrap on narrow screens; >3 labeled buttons usually wrap. Prefer short labels or emoji-only + clear alt via tab title in header.
- Section accessory buttons can squeeze text; keep Section text short (one line title + one line subtext).
- Label `description` position differs by platform (above/below) **[D]**.
- Wide tables in code blocks scroll horizontally on mobile; render them as images instead.
- Thumbnails are tiny on mobile; don't put information-bearing text in them.
- Test light and dark themes for accent colors and generated images. The client theme is not exposed to bots, so use mid-contrast palettes or opaque card backgrounds that read on both.

### 6.9 State & lifecycle

- Interaction tokens last 15 min; for persistent dashboards rely on bot-token `Edit Message` instead.
- Stateless handlers: everything needed to re-render must be recoverable from `custom_id` + store + `interaction.message`.
- Concurrency: two users clicking the same public dashboard → last write wins; include a version in the custom_id/store and re-render from fresh state.
- Use ephemeral (`32832`) for per-user settings panels to avoid fights over a shared message.

---

## 7. `custom_id` design

Hard constraints **[D]**: string, 1-100 characters, unique within the message, echoed verbatim; not secret (any client can see and forge it — **always re-authorize server-side**).

### 7.1 Recommended format **[R]**

```
<ver><SEP><route><SEP><arg1><SEP><arg2>...
v1|dash|tab|2
v1|ban|cf|8kFq2|1Hk9zQ         (confirm ban: nonce + base62 snowflake)
v1|s|Q2x9            (store reference)
```

- `ver` (1-2 chars) lets you change encoding without breaking live messages.
- `route` = short registered handler key (vesper maps `"dash"` → handler at compile time; collision check).
- Separator: a char that can't appear in args (`|` or `\u001F`-like printable alternatives such as `:`); escape or forbid in args.

### 7.2 Packing techniques

| Technique | Saves | Example |
|---|---|---|
| Snowflake → base62 | 19-20 digits → ~11 chars | `1234567890123456789` → `1TCKi1nFuNh` |
| Ints → base62/base36 | variable | page 1234 → `jU` |
| Enum → 1 char | words → 1 | tab `members` → `m` |
| Bitflags → base62 | many bools → few chars | 30 toggles in 6 chars |
| Positional args | no keys | `v1|pg|3|25` not `page=3&size=25` |
| Binary + base64url/base85-ish (printable) | dense | serde/postcard bytes → base64url (4/3 overhead) |

Budget rule of thumb: 100 chars ≈ 75 raw bytes with base64url, ≈ 6 snowflakes in base62.

### 7.3 Store references

When payload > ~80 chars, or contains sensitive or mutable data:

- Persist state server-side (`moka`/Redis/SQL) keyed by a short random id (e.g. 8-12 base62 chars, ~48-71 bits), with TTL (≥15 min for interaction-bound, longer for persistent dashboards).
- `custom_id = v1|s|<key>|<action>`; the key resolves to the session (view, filters, page, owner user id).
- Expired key → reply ephemeral "This panel expired" + a "Reopen" button. Keep enough inline data (route + target) to reopen.
- Optional HMAC truncated tag (e.g. 6-8 chars) to detect forged/tampered stateless ids.

### 7.4 Modal custom_ids

Modal `custom_id` (1-100) plus each field's `custom_id` (1-100). Put the session reference on the modal; field ids can be short constant names (`"log_ch"`).

---

## 8. Implications for vesper

### 8.1 Typed component model

- Encode placement rules in types so illegal trees don't compile:
  - `MessageTopLevel = Container | Section | TextDisplay | MediaGallery | File | Separator | ActionRow`
  - `ContainerChild = ActionRow | TextDisplay | Section | MediaGallery | Separator | File` (no `Container`)
  - `SectionAccessory = Button | Thumbnail`; `Section { texts: Bounded<TextDisplay, 1, 3>, accessory }`
  - `ActionRow = Buttons(Bounded<Button, 1, 5>) | Select(AnySelect)` (enum makes "one select alone" unrepresentable to violate)
  - `ModalTopLevel = Label | TextDisplay`; `LabelChild = TextInput | StringSelect | UserSelect | ... | Checkbox`
  - Separate `MessageSelect` (has `disabled`) vs `ModalSelect` (has `required`, no `disabled`) types.
- Button style as enum carrying its required data: `Button::Action { style: ActionStyle, custom_id, label, emoji, disabled } | Button::Link { url, .. } | Button::Premium { sku_id }`.
- `#[non_exhaustive]` + `Unknown(serde_json::Value)` for forward compatibility on deserialization.

### 8.2 Builder API sketch

```rust
let view = Message::v2()
    .ephemeral(true)
    .allowed_mentions(AllowedMentions::none())
    .container(|c| c
        .accent(0x5865F2)
        .section(|s| s.text("## Server Dashboard").text("-# updated <t:1792400000:R>")
                       .thumbnail(Media::attachment("icon.png").alt("Server icon")))
        .tabs(Tabs::new(Route::Dash).active(0).tab("Overview").tab("Members").tab("Logs"))
        .separator(Spacing::Small)
        .gallery(|g| g.item(Media::attachment(chart.filename()).alt("Messages per day")))
        .paginated(items, |item, row| row.section_button(item.title(), Btn::secondary("Configure", Route::Open(item.id))))
    )
    .attach(chart)                // vesper tracks attachment:// refs and the `attachments` array
    .build()?;                    // -> Result<Validated<Message>, LimitError>
```

- `Route` = typed enum with `#[derive(CustomId)]` generating encode/decode, length checks, and version tags; handler dispatch via the same enum.
- `Tabs`, `Pager`, `Wizard`, `ConfirmDialog` as higher-level widgets that expand into components and report their cost.

### 8.3 Limit validation

| Check | Compile time | Run time |
|---|---|---|
| Placement / nesting legality | yes (types) | – |
| Buttons ≤5 per row, Section texts 1-3 | yes with const generics / `Bounded<T, MIN, MAX>` for static rows; run time for iterator-built rows | yes |
| Literal string lengths (labels, placeholders, titles) | yes via `const fn` checks or proc-macro on literals (`label!("…")`) | yes for dynamic |
| `custom_id` ≤100 | proc-macro computes max encoded length of a `Route` variant (fixed-width fields ⇒ provable bound) | yes (fallback to store ref) |
| Unique `custom_id`s & `id`s | – | yes (HashSet pass) |
| 40-component total | partly (const cost of static widgets) | yes, precise tree walk |
| 4000-char total text | – | yes (count Unicode scalar values; treat as configurable `Limits` struct) |
| `attachment://` refs resolve to attached files; no orphan attachments | – | yes |
| Modal: 1-5 top-level, no `disabled`, `min_values`≥1 when `required` | types + runtime | yes |
| `default_values` count in `[min,max]` | – | yes |

Return rich errors (`LimitError { path: "components[0].components[3].accessory.label", limit, actual }`) and offer a `debug_assert` + `strict`/`lenient` (auto-truncate with `…`) policy.

### 8.4 Auto-pagination

- `Paginator<T>` takes items + a per-item renderer, computes each item's `(components, chars)` cost, reserves chrome cost, and packs pages greedily under `Limits { components: 40, text_chars: 4000 }`.
- Page state encoded in custom_id (`v1|pg|<key>|<n>`) or store; handler auto-registered; responds with `UPDATE_MESSAGE`.
- Fallback ladder when a single item is too big: truncate text → move to image render → "View details" button opening an ephemeral follow-up.

### 8.5 Interaction runtime

- Auto-ACK policy: if handler hasn't responded in ~2.5 s, send `DEFERRED_UPDATE_MESSAGE` (component) or `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` (command, EPHEMERAL only) and switch to edit-original, adding the V2 flag there.
- Always set `flags` to include `IS_COMPONENTS_V2` on every edit of a V2 message; never send `content`/`embeds` for V2.
- Attachment manager: diff previous vs new `attachment://` refs, emit `attachments` keep-list + new `files[n]`, content-hash filenames.
- Webhook sender appends `?with_components=true` automatically and rejects interactive components for non-owned webhooks.
- Modal submit parser: typed extraction by field `custom_id` (`submit.get::<ChannelSelect>("log_ch")`), joined with `resolved`; download File Upload attachments immediately (ephemeral URLs).
- Snapshot testing: serialize built views to JSON and assert against fixtures; a `cost()` report in tests to keep dashboards under budget.

### 8.6 Open questions to verify empirically

1. Exact 4000-char counting unit (UTF-16 code units vs code points) and whether markdown syntax counts (it does, as raw string). **[C]**
2. Per-Text-Display max length (some libraries assume 2000; Discord docs specify none). **[C]**
3. Max `url` length for Media Gallery/Thumbnail in messages (2048 documented only for Component Embeds).
4. Whether `UPDATE_MESSAGE` from a modal submit can switch a legacy message to V2 (Edit Message allows setting the flag).

---

## 9. Patterns from schwi-v2 (practical reference)

`schwi-v2/apps/discord-bot` is a production TypeScript bot on `discord.js` + **tescord** (`tescord@0.0.6`, a thin "Pack" framework). Its Components V2 UIs (setup dashboards, temp-voice control panel, payload/component editor, punishment history) are the practical model for vesper. Files studied (paths relative to `/home/user/schwi-v2/apps/discord-bot/`): `src/modules/welcome/setup.ts`, `src/modules/temp-voice/{setup,trigger}.ts`, `src/modules/partnership/setup.ts`, `src/modules/punishHistory/commands.ts`, `src/modules/seacord/warn.ts`, `src/integrations/payload/editor/{generatePayloadEditor,payloadButtons}.ts`, `src/integrations/payload/editor/container/index.ts`, `src/types/components.d.ts`. (`components_v2.xml` and `components.txt` are just copies of the Discord docs for AI context; they hold no extra patterns, and the XML is out of date: it still lists the old 10/30 limits.)

### 9.1 Architecture: pure `render(state) -> components`, handlers mutate then re-render

Each screen is **one async render function** that loads state from the DB and returns the whole top-level tree. Every handler (button, select, modal) does **ACK → mutate DB → re-render the same screen → edit**. Nothing is diffed or patched; the message is regenerated each time.

```ts
// src/modules/welcome/setup.ts
async function renderWelcomeSetup(guild, locale, selectedSystemId?: number) {
  const welcomeSystems = await prisma.welcomeSystem.findMany({ where: { guild_id: guild.id }, take: 25 });
  ...
  return [{ type: ComponentType.Container, components: [
      { type: ComponentType.Section,
        components: [{ type: ComponentType.TextDisplay, content: "# " + locale.welcome.setup.title() }],
        accessory: { type: 11, media: { url: "https://cdn.discordapp.com/attachments/.../raw.png" } } },
      ...systemSelectorComponents, ...configComponents, ...managementComponents ] }];
}

pack.channelSelectMenu({ id: "welcome-setup-channel-select",
  handle: async ({ interaction, data, locale }) => {
    const [system, systemId] = data;
    await interaction.deferUpdate();                                   // ACK, no loading state
    await prisma.welcomeSystem.update({ where: { id: +systemId }, data: { channel_id: interaction.values[0] ?? null } });
    await interaction.editReply({ components: await renderWelcomeSetup(interaction.guild, locale.user, +systemId),
                                  flags: MessageFlags.Ephemeral | MessageFlags.IsComponentsV2 });
  }});
```

**vesper:** make this the core model: `trait View { type State; async fn render(&self, ctx, state) -> Message }` with handlers returning `Update::Rerender(state)`. The framework does the defer/edit and always adds the V2 flag (schwi repeats `flags: Ephemeral | IsComponentsV2` by hand in ~every handler).

### 9.2 Container layout conventions

Recurring skeleton (welcome, temp-voice, partnership setup):

1. **Header Section**: `# Title` Text Display + **Thumbnail accessory** (module icon).
2. **Entity switcher** (only shown if >1 entity): Text Display caption + String Select of entities (`default: true` on current).
3. **Field blocks**: each setting = a caption Text Display + an Action Row with one select (channel/role/payload), using `defaultValues`/`default` to show the current value.
4. **Scalar settings as Sections**: text with the current value inline + an accessory button "Set"/"Edit". The button is `Primary` when unset and `Secondary` when set (calls attention to missing config).
5. **Management Sections** at the bottom: "Add new …" (`Primary`) and "Delete current …" (`Danger`) as Section accessories.
6. **Separators** between blocks (`spacing: Large` before major sections, `Small` inside).

```ts
// Scalar setting row (welcome/setup.ts)
{ type: ComponentType.Section,
  components: [{ type: ComponentType.TextDisplay,
                 content: label + (sys.morph_in_seconds > 0 ? ` (${sys.morph_in_seconds}s)` : "") }],
  accessory: tescord.buildComponent({ id: "welcome-setup-seconds-modal",
    data: ["welcome", sys.id.toString(), "morph_in_seconds"],
    overrides: { label: sys.morph_in_seconds > 0 ? t.edit() : t.set(),
                 style: sys.morph_in_seconds > 0 ? "Secondary" : "Primary" } }) }

// Toggle row (partnership/setup.ts): style encodes the action, not the state
accessory: tescord.buildComponent({ id: "partnership-setup-toggle-enabled",
  overrides: { label: enabled ? t.disable() : t.enable(), style: enabled ? "Danger" : "Success" } })
```

**vesper helpers:** `Header::new(title).thumbnail(url)`, `EntitySwitcher` (auto-hidden when ≤1), `Field::channel(..)/role(..)/choice(..)` (caption + prefilled select), `SettingRow::value(label, Option<T>).edit(route)` (auto Primary/Secondary), `Toggle::new(label, bool, route)` (auto Success/Danger + label), `DangerRow`.

### 9.3 Navigation

- **Entity tabs via select**: switching the edited entity re-renders with `selectedSystemId`; the select keeps the current item as `default`.
- **Drill-down + Back**: the payload editor lists sub-components in String Selects (value `edit-component:<id>`); choosing one replaces the message with a component-specific editor, which has `back-to-container-editor` / `back-to-payload-editor` / `payload-editor-back` buttons that re-render the parent view. Breadcrumb state is only the ids in `data`, so there's no server-side navigation stack.
- **Sub-panel as ephemeral reply**: on the public temp-voice control panel, "Allow list" opens an **ephemeral** reply (`deferReply({ephemeral:true})` + `editReply`) holding a User Select. Its custom_id carries `[voiceProfileId, interaction.message?.id]` so the handler can find and refresh the parent public message.

**vesper:** a `Nav` stack encoded in routes (`Back(Box<Route>)` or parent route id), `DrillDown` select helper, and `open_ephemeral_subpanel(parent_msg)` that wires parent refresh automatically.

### 9.4 Pagination

Three variants, all stateless (page number in the custom_id):

```ts
// punishHistory/commands.ts: DB-offset paging, 25 rows rendered into ONE Text Display
take: 25, skip: page * 25
{ type: ActionRow, components: [
  tescord.buildComponent({ id: "punish-history-set", data: [userId, guildId || 0, page - 1],
                           overrides: { disabled: page <= 0, label: t.prevPage() } }),
  tescord.buildComponent({ id: "punish-history-set", data: [userId, guildId || 0, page + 1],
                           overrides: { disabled: punishments.length < 25, label: t.nextPage() } }) ] }
```

- **Same handler id, different data**: Prev and Next share `id` and differ only in the encoded page, which keeps custom_ids unique.
- **"Has next" heuristic** = page full (`length < 25` ⇒ last page), so no COUNT query is needed (it fails when the total is an exact multiple of 25).
- **Settings split into fixed pages** (`temp-voice/setup.ts`: `maxPages = 2`, `switch (currentPage)`, Prev/Next with custom emoji, clamped page) when one entity's settings exceed the 40-component budget.
- **Select chunking** (payload editor): >25 items → several String Selects of 25, placeholder "Components 26-50".
- **Budget fallback**: `generatePayloadEditor` falls back to a simpler editor if the tree is too big. Bug: it checks `fullEditor.components.length > 40`, which only counts top-level components, so nested ones are missed and the check can pass while the real total is over 40. vesper should count recursively (§8.3).

**vesper:** `Pager { page, has_next }` widget (auto-disable, same route with page arg), `SettingsPages` for static multi-page forms, `ChunkedSelect` (auto split by 25 with range placeholders), and recursive budget counting with a declared fallback view.

### 9.5 Step flows / modals

- Button → `showModal` with the entity id in the modal's custom data; modal handler validates input, writes the DB, re-renders the dashboard (or replies with an ephemeral error).
- Older modals still use **Action Row + Text Input with `label`**, which is deprecated (`temp_voice_limit_modal`). vesper should emit only Label-wrapped inputs.
- Validation is manual (`parseInt`, range check, then an `invalidNumber()` reply). vesper can derive it from typed modal fields (`#[field(range = 0..=99)] limit: u8`).
- Wizard-like setup is modelled as a **single dashboard** with Primary "Set" buttons on missing fields rather than a linear wizard. That works well for re-editable config, so vesper should offer both `Dashboard` and `Wizard`.

### 9.6 custom_id / data passing (tescord encoding)

tescord's `buildComponent({ id, data, overrides })` encodes `id + SPLIT + data.join(SPLIT)`:

```js
// tescord/dist/lib/types/ComponentBuilder.js
CUSTOM_DATA_SPLITTER = "䲜";          // rare CJK char = 1 char, never in user data in practice
CUSTOM_DATA_NUMBER_INDICATOR = "㥻";  // prefix marks numbers -> decoded back to Number
// strings raw, numbers "㥻"+n, other -> JSON.stringify; encode/decode hooks for custom types
```

- Handlers are registered by base `id`; `data` arrives typed-ish as an array (`const [system, systemId, fieldKey] = data`). A global `Tescord.ComponentMap` (`src/types/components.d.ts`) maps each id to its component kind for type safety. vesper's `Route` enum + derive gives the same thing with real types.
- **Cache-busting nonce**: selects are given `Math.random().toString(36).substring(2)` as extra data on every render. The changing custom_id makes the client drop its local selection state and show the server's `default`/`defaultValues` after a re-render. vesper should do this automatically for selects in re-rendered views (e.g. a 2-3 char render counter), within the 100-char budget.
- **Risky usage**: `seacord-warn-dm-fallback` puts the free-text `reason` and several numbers in the custom_id. That can exceed 100 chars, or contain the splitter. vesper should reject unbounded strings in route fields at compile time and push them to the store (§7.3).
- Ids are long kebab strings (`auto-partnership-setup-partnership-request-channel-select`, 56 chars). Fine with tescord, but they leave little room for data. vesper should use short route tags.

### 9.7 Update vs reply

| Situation | schwi-v2 does | vesper default |
|---|---|---|
| Slash command opens dashboard | `deferReply({flags: Ephemeral\|IsComponentsV2})` then `editReply(render())` | Auto: send type 4 if render < ~2.5 s, else defer (EPHEMERAL only) and edit with the V2 flag |
| Component changes dashboard | `deferUpdate()` + `editReply(render())` | `UPDATE_MESSAGE` (7) directly when fast, otherwise 6 + edit |
| Sub-panel / permission-gated action | `deferReply({ephemeral:true})` + `editReply` / `followUp` | `reply_ephemeral(view)` |
| Error during component handling | `followUp({components:[TextDisplay], flags: V2\|Ephemeral})` | `Err(UserError)` → framework sends ephemeral V2 error |
| Modal open | `showModal` (must be the first response, so no defer before) | Type-check: modal routes can't be preceded by a defer |
| DM to user who may not render V2 | Sends V2 + a fallback button that re-sends as plain `content` | Optional `LegacyFallback` (text rendering of a view) |

### 9.8 Error and empty states

- **Empty list → disabled placeholder select**: `options: [{ label: "No payloads available", value: "none", description: "Create a payload first", default: true }], disabled: true`, which keeps the layout stable. (Note: `"none"` must then be handled as "clear" in the handler, which schwi does.)
- **Empty text**: `noneFound()`, `noAllowedMembers()` and `noComponents` replace the content instead of hiding the block.
- **Errors**: one-line ephemeral V2 Text Display (`voiceProfileNotFound`, `noPermission`). Some old handlers still reply with legacy `content` and no V2 flag, which is inconsistent.
- **Destructive success**: after delete, the panel is replaced by a single Text Display ("Payload X deleted").
- **Render failures**: `try/catch` around render → simplified fallback view → final "loading error" text.
- **Auth re-checked in every handler** (owner/moderator check before acting), never trusted from the custom_id. vesper should keep this.

### 9.9 What vesper should build in (summary)

1. `View`/`render` + automatic ACK/defer/edit with V2 + ephemeral flags handled by the framework.
2. Layout widgets: `Header`, `EntitySwitcher`, `Field::*` (caption + prefilled select), `SettingRow`, `Toggle`, `DangerRow`, `EmptyState`, `ErrorNotice`.
3. Navigation: `Back`/drill-down routes, ephemeral sub-panels that refresh their parent message.
4. Pagination: `Pager`, `SettingsPages`, `ChunkedSelect`, recursive 40-component / 4000-char budget with a declared fallback view.
5. Typed routes (`#[derive(Route)]`) replacing tescord's splitter strings: compile-time max-length, automatic select nonce, store-backed fields for unbounded data.
6. Modals: Label-only, typed fields with validation, modal-open routes that can't be deferred.
7. Optional legacy text fallback for DM recipients on clients without V2 support.

---

### Secondary sources

- [discord.js guide: Display Components](https://discordjs.guide/popular-topics/display-components) (4000-char total, nested components count toward 40)
- [JDA TextDisplay javadoc](https://docs.jda.wiki/net/dv8tion/jda/api/components/textdisplay/TextDisplay.html) (no per-component length limit; 4000 total)
- [Discord.Net Components V2 advanced guide](https://docs.discordnet.dev/guides/components_v2/advanced.html)
- [Discord App Best Practices: UI/UX for Developers (video)](https://www.youtube.com/embed/gIYIvVkWdvk)
- Change log entries: [2025-04-22 Components V2 launch](https://discord.com/developers/docs/change-log), 2025-04-29 raised limits (40 total, no top-level/container caps), 2025-08-25 Label + String Select in modals, 2025-09-10 all selects + Text Display in modals, 2025-10-15 File Upload, 2026-02-12 Radio/Checkbox Group/Checkbox, 2026-08-05 `file_types`, 2026-09-03 20 MiB uploads, 2026-10-05 Component Embeds.
