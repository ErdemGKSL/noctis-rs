---
title: 'Story 9.5: Views over webhooks'
type: 'feature'
created: '2026-10-22'
status: 'done'
baseline_revision: 'd6d8779f0f53566a85a2e4787f60d4eab190d007'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/traits-and-generics.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Noctis has no webhook client. A bot author cannot send or edit a View through a webhook (log channels, personas, cross-posting) with the V2 safety interaction replies get: components are silently dropped without `?with_components=true` (WH-01), interactive components through a non-owned webhook fail (WH-02), legacy fields must be nulled on edit (WH-03), names are 1–80 (WH-04), forum/media channels need a thread (WH-05), and only some flags are valid per endpoint (MSG-07) (FR-65; AD-11, AD-12).

**Approach:**
- `noctis-ui` gets a non-interactive V2 message type, `StaticMessage`, built from static tree builders whose placement traits exclude buttons with a `custom_id` and selects (a `[WH-02]` compile error). It also gets a runtime `TryFrom<V2Message>`, a checked `WebhookName` with `webhook_name!`, and a webhook edit body that nulls the legacy fields.
- `noctis-core` gets `ctx.webhooks()` / `app.webhooks()`, which return a `Webhooks` factory. It creates or wraps `Webhook<Owned>` and `Webhook<Foreign>` handles, whose sends and edits go through the Responder's own preparation (key resolution, Route ids and Refs, validation, the Upload Budget and attachments). They always send `with_components=true` and flags typed per endpoint, and check WH-05 before any request.

## Boundaries & Constraints

**Always:**
- **Rules (`noctis-rules`, `src/message.rs`, next to `validate_webhook_name`):**
  - **WH-04, extended:** `validate_webhook_name(name, limits)` keeps 1..=`webhook_name_len` (by `discord_len`, path `name`). It adds one violation `Bound::Describe("a name without `clyde` or `discord` (any case)")`, `Actual::Text(name)`, when the ASCII-lowercased name contains `clyde` or `discord` (Discord's Create Webhook rule). The existing `wh_04_webhook_name` tests stay; new cases are added.
  - **New `Limits` field** `webhook_thread_name_len = 100, "WH-05", "characters in a webhook thread_name."`. Update the `limits.rs` field-list test and the skill's `limits-cheatsheet.md`.
  - **New `pub enum WebhookThreadDecl<'a> { None, Id, Name(&'a str) }`** (`#[derive(Debug, Clone, Copy, PartialEq, Eq)]`, `#[non_exhaustive]`).
  - **New `pub fn validate_webhook_thread(thread_only: bool, thread: WebhookThreadDecl<'_>, limits: &Limits) -> Vec<Violation>`.** `thread_only` means a forum or media channel. It reports:
    - `thread_only` with `None`: WH-05 at path `thread_id`, `Describe("a thread_id or a thread_name in a forum or media channel")`;
    - a `Name` with 0 or more than `webhook_thread_name_len` characters: WH-05 `Between{1,100}` at path `thread_name`, in any channel;
    - `!thread_only` with `Name(_)`: WH-05 at `thread_name`, `Describe("a thread_name only in a forum or media channel")`;
    - `Id` is always fine.
  - **New `pub fn validate_static_components(components: &[Component<'_>]) -> Vec<Violation>`** (WH-02). One violation per Button whose style is not `Link` (Premium counts as interactive) and per `Select`, at any depth: rows, Section accessories and Container children. Path format as in `validate_components` (for example `components[0].components[1].accessory`), `Describe("a non-interactive component (a link button, text, media, file, separator, container or section)")`, `Actual::Text(kind)` (`button` / `select`). `Opaque` is skipped, because raw JSON is checked in `noctis-ui`.
- **`noctis-ui` (new `src/message/static_msg.rs`, re-exported at the crate root):**
  - **Static builders:** `StaticComponents`, `StaticContainer`, `StaticSection` and `StaticActionRow`, each `#[derive(Debug, Clone)]`, `#[must_use]` builders.
    - Push methods are bounded by new sealed traits, each with `#[diagnostic::on_unimplemented(message = "[WH-02] `{Self}` …")]` and a note pointing to `StaticMessage::try_from` and owned webhooks.
    - `StaticTopLevel` is implemented by `StaticContainer`, `StaticSection`, `StaticActionRow`, `TextDisplay`, `MediaGallery`, `File` and `Separator`.
    - `StaticContainerChild` is the same set minus `StaticContainer`.
    - `StaticAccessory` is implemented by `Thumbnail` and `LinkButton`; `StaticRowButton` by `LinkButton`.
    - Never implemented: `RouteButton<R>`, `Button`, `PremiumButton`, any select or `StringSelectBuilder`, `ActionRow`, `Container`, `Section`, `TopLevelComponent`, `ContainerComponent`, `RawComponent`.
  - **Constructors:** `StaticSection::new<A: StaticAccessory, const N>(texts: [TextDisplay; N], accessory) where Arity<N>: SectionTexts` and `StaticActionRow::buttons<B: StaticRowButton, const N>([B; N]) where Arity<N>: RowButtons`. `StaticContainer` has `new`, `push`, `accent(Rgb)` and `spoiler(bool)`; `StaticComponents` has `new`, `push`, `len` and `is_empty`. Each converts into the existing erased node through `From`, so serialization, numbering, lowering, `validate` and `cost` are reused unchanged.
  - **`StaticMessage`** (`#[derive(Debug, Clone)]`) wraps a private `V2Message`:
    - `new(StaticComponents)`, `attachments(AttachmentSet)` and `mentions(Mentions)` (`#[must_use]`);
    - `as_v2(&self) -> &V2Message`;
    - `From<StaticComponents> for StaticMessage`, `From<StaticMessage> for V2Message`;
    - `TryFrom<V2Message> for StaticMessage` (`Error = UiError`): `Err(UiError::Limits{violations})` with every WH-02 violation, from `validate_static_components` over the lowered tree plus raw JSON, never partial.
  - **Raw JSON walk:** a hidden helper in `component/raw.rs` uses the same walk as the CV2-16 check (`components`/`accessory`/`component`). It flags an object with integer `type` in {3,5,6,7,8}, or `type` 2 with `style` other than 5, as WH-02 at `<raw path>.<json path>`.
  - **`IntoStaticMessage`:** a sealed trait implemented only for `StaticMessage` and `StaticComponents`, with `on_unimplemented(message = "[WH-02] `{Self}` may hold interactive components: a webhook the app does not own sends only a `StaticMessage`", note = "build it with StaticComponents/StaticContainer/StaticSection/StaticActionRow, or convert at run time with StaticMessage::try_from(msg)")`.
  - **`WebhookName`** is a `literal_newtype!` in `literal.rs` over `validate_webhook_name` (`try_new`, `as_str`, `TryFrom<&'static str>`/`String`, hidden `__from_checked`).
  - **`V2Message::webhook_edit_body(&self, flags: EditFlags) -> MessageBody<'_>`** serializes `flags` (with `IS_COMPONENTS_V2`), `content: null`, `embeds: []`, `poll: null`, `components` and `allowed_mentions`. It sends no `sticker_ids`, which is not an Edit Webhook Message parameter (WH-03). Add a `MessageBody` inner variant or flag for it.
- **`noctis-macros`:** `webhook_name!` (`literal.rs` `TextMacro { name: "webhook_name", rule: WH_04, ty: "WebhookName", check: validate_webhook_name }`, `lib.rs` proc macro), re-exported as `noctis::webhook_name` and in the prelude macro list.
- **`noctis-core` (new `src/webhook.rs`, plus `src/webhook/tests.rs`):**
  - **`Webhooks`** (`Clone`, `Debug`) holds `Option<Arc<AppShared>>`. It is reached through `webhooks()` on `CommandCtx`, `EventCtx`, the `ctx_accessors!` contexts (`ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx`) and `App`. A detached context returns `Err(WebhookError::Detached)` from every send and sends nothing.
    - `async fn create(&self, channel: WebhookChannel, name: WebhookName) -> Result<Webhook<Owned>, WebhookError>` sends `Route::CreateWebhook` with the bot token and body `{"name"}`. It parses `id`, `token` and `channel_id`; a missing or invalid one is `Transport`. The handle carries the channel's kind.
    - `owned(id: Id<WebhookMarker>, token: impl Into<String>) -> Result<Webhook<Owned>, WebhookError>` and `foreign(..) -> Result<Webhook<Foreign>, _>` wrap stored credentials. A token that fails `edge::callback::valid_token` is `InvalidToken`, and nothing is sent.
  - **`WebhookChannel`** (`Copy`): `new(Id<ChannelMarker>, ChannelType)`, `From<&InteractionChannel>` (twilight's resolved channel), `id()`, `kind()`.
  - **`Webhook<O: Ownership>`**:
    - `Ownership` is sealed, with markers `pub enum Owned {}` and `pub enum Foreign {}`; the aliases are `OwnedWebhook`/`ForeignWebhook`.
    - Accessors: `id()`, `token() -> &str` (documented as a secret to store), `channel_kind() -> Option<ChannelType>`, and `#[must_use] in_channel(ChannelType) -> Self`, which declares the kind for stored credentials.
    - `Debug` redacts the token. The token is never in a log, a span field or an error.
  - **Sends:** `Webhook<Owned>::send(&self, msg: impl Into<V2Message>)` and `Webhook<Foreign>::send(&self, msg: impl IntoStaticMessage)` return the `IntoFuture` builder `ExecuteWebhook<'_, O>`, with `flags(CreateFlags)`, `username(WebhookName)`, `avatar_url(impl Into<String>)` and `thread(WebhookThread)`; it resolves to `WebhookMessage<O>`.
  - **`WebhookThread`** is `Existing(Id<ChannelMarker>) | Create(Cow<'static, str>)`, with `From<Id<ChannelMarker>>`. An enum, so a send can never carry both.
  - **Edits and deletes by id:** `edit(&self, id: Id<MessageMarker>, msg)` (Owned: `impl Into<V2Message>`; Foreign: `impl IntoStaticMessage`) returns `EditWebhookMessage<'_, O>`, with `flags(EditFlags)` and `thread(Id<ChannelMarker>)`. `delete(&self, id)` returns `DeleteWebhookMessage<'_, O>`, with `thread(Id)`. An edit by id has no known files: every used attachment is uploaded and the rest are dropped (documented).
  - **`WebhookMessage<O>`** (`Debug`): `id()`, `channel_id()`, `thread_id() -> Option<Id<ChannelMarker>>`. Its `edit(msg)` (same message bound as `O`) diffs attachments against its `KnownFiles` and passes its thread; `delete()` passes its thread. The thread is the `Existing` id, or the response `channel_id` for `Create`.
  - **One preparation path.** Each send builds a fresh `Shared` through a new crate-private `Shared::webhook(app: Arc<AppShared>)`:
    - the new `Transport::Webhook(Arc<AppShared>)` arm returns `app` from `signer()`/`ref_target()` and `None` from `file_limit()`;
    - its interaction-only `Transport` methods behave like `Detached` (they are never called);
    - its i18n is the App's `strings` with no locales (`en-US`), visibility `Public`.
    - It calls `prepare` (keys, Route ids and Refs, `noctis_ui::validate`) and then `body(.., known, None)`, so the Upload Budget, uploads (`files[n]`) and the keep-list are the Responder's.
  - **Wire requests** (all without the bot `Authorization` header, through the App's twilight client):
    - **Execute:** `Route::ExecuteWebhook { wait: Some(true), with_components: Some(true), thread_id }`, body `create_body(flags)`. When set, `username`, `avatar_url` and `thread_name` are inserted.
    - **Edit:** `RequestBuilder::raw(Method::Patch, "webhooks/{id}/{token}/messages/{mid}?with_components=true[&thread_id={t}]")`, body `webhook_edit_body(flags)` plus the `attachments` keep-list. twilight's `UpdateWebhookMessage` route has no `with_components`.
    - **Delete:** `Route::DeleteWebhookMessage`.

    Response parsing reuses `transport.rs`'s `posted`/`message_id`/`http_error`; make them `pub(crate)`.
  - **Checks before any request, in this order:**
    1. token;
    2. WH-05 when `channel_kind` is known (forum = `GuildForum`, media = `GuildMedia`; for edit/delete by id, `WebhookThreadDecl::Id`/`None` from `.thread`);
    3. `prepare`;
    4. body.

    `with_components=true` is sent on every execute and edit, and there is no setter for it.
  - **Error mapping.** Discord answering 400 with code 220001 or 220003 becomes `WebhookError::Invalid(UiError::Limits{[WH-05 violation]})`, which covers unknown channel kinds. Any other non-2xx is `Rejected{status, code}`.
  - **`WebhookError`** (`#[non_exhaustive]`, `thiserror`): `Invalid(#[from] UiError)`, `Encode{reason}`, `RefStore(PortError)`, `RefCollision`, `InvalidToken`, `Rejected{status: u16, code: Option<u32>}`, `Transport`, `Detached`. A private `From<ResponderError>` maps the shared variants; any other variant maps to `Transport` with a `debug` log.
- **Facade (`crates/noctis/src/lib.rs`):**
  - a new `pub mod webhook` with module docs and an example: `Webhooks, Webhook, OwnedWebhook, ForeignWebhook, Owned, Foreign, Ownership, WebhookChannel, WebhookThread, WebhookMessage, WebhookError, ExecuteWebhook, EditWebhookMessage, DeleteWebhookMessage`;
  - `noctis::ui` gains `StaticMessage, StaticComponents, StaticContainer, StaticSection, StaticActionRow, StaticTopLevel, StaticContainerChild, StaticAccessory, StaticRowButton, IntoStaticMessage, WebhookName`;
  - `noctis::rules::message` gains `validate_webhook_thread`, `WebhookThreadDecl` and `validate_static_components` (all three live in `noctis-rules/src/message.rs`, re-exported through the existing `rules::message` module path);
  - the prelude adds `webhook_name`.
- **Docs, template, skill and gates** (same change):
  - **`docs/discord-rules.md`, How column:**
    - WH-01: always on execute and edit, no setter, `compile_fail` doctest;
    - WH-02: tier becomes `Compile-time / Runtime`, with the static traits, the fixtures, `try_from` and the raw walk;
    - WH-03: `webhook_edit_body`, `StaticMessage` has no `content`;
    - WH-04: `webhook_name!`, `WebhookName`, the clyde/discord check;
    - WH-05: `validate_webhook_thread`, 1–100 `thread_name`, the 220001/220003 mapping;
    - MSG-07 notes the webhook endpoints.
  - **`xtask/rules-pending.toml`:** remove the five WH entries. Evidence:
    - WH-01: a `compile_fail` doctest line `let _ = webhook.send(msg).with_components(false); // [WH-01]`;
    - WH-03: `StaticMessage::new(StaticComponents::new()).content("x"); // [WH-03]`;
    - WH-02/WH-04: the fixtures below;
    - WH-05 and WH-02 runtime: unit tests named `wh_05_*`/`wh_02_*`.
  - **Architecture spine:** an "Amended (Story 9.5)" bullet under AD-11.
  - **`xtask/src/skill_check/components.rs` `BUILDERS`:** add `StaticContainer`, `StaticSection` and `StaticActionRow`, each with a `components-v2.md` entry (**Where:**, **Limits:**, **Builder:**), and a "foreign webhook (`StaticMessage`)" column in the placement matrix.
  - **The skill:**
    - rewrite `responder.md` §7 (drop `[planned]`): owned vs foreign, persona, threads, handles, errors, the testing recipe with `http_fake` routes;
    - `components-v2.md` §15 (the new rule functions);
    - `limits-cheatsheet.md` rows WH-01 and WH-05 plus the new Limits field;
    - `troubleshooting.md` (`[WH-02]`, `[WH-04]`);
    - `SKILL.md` L364 row and the API hash;
    - `AGENTS.md` of the template mentions `ctx.webhooks()`.
  - **Template:** see Tasks.
  - Re-bless `xtask/public-api.txt`.

**Never:**
- New crate dependencies or new rule ids (only the WH-04 and WH-05 rows change).
- Legacy (`LegacyMessage`) webhook messages, `applied_tags`, `tts`, `poll`, embeds, the webhook GET/modify/delete-webhook endpoints, webhook URL parsing, Slack/GitHub-compatible endpoints.
- A static form of `components!`; use `StaticMessage::try_from`.
- `with_components` on interaction follow-ups (INT-13 stays).
- A bot `Authorization` header on webhook-token routes.
- Logging or `Display`-ing a webhook token or path.
- Truncating a name or a thread name in Lenient mode.
- Editing `_bmad/` or weakening `.cargo/config.toml`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error |
|---|---|---|---|---|
| N1 | Name bounds | `WebhookName::try_new` with `"a"`, 80×`a`, 40×`😀` (`discord_len` 80); `""`, 81×`a`, 41×`😀` | ok; WH-04 `Between{1,80}` | `UiError::Limits` |
| N2 | Reserved words | `"Clyde"`, `"my discord log"`, `"DISCORD"`, `"clyd e"`, `"Díscord"` | WH-04 Describe ×3; ok ×2 (ASCII lowercase only) | `UiError::Limits` |
| N3 | Literal | `webhook_name!("Audit log")`; `webhook_name!("")`; 81 chars; `webhook_name!(x)` | `WebhookName`; compile errors `[WH-04] …` (fixtures) | compile error |
| N4 | Persona | `.username(webhook_name!("Mod log")).avatar_url("https://a/b.png")` | body has `"username":"Mod log","avatar_url":"https://a/b.png"` | — |
| E1 | Owned execute | owned, `send(V2Message with a TextDisplay)` | `POST /webhooks/{id}/{tok}?wait=true&with_components=true`, no `Authorization`, body `flags` 32768, `components`, `allowed_mentions {"parse":[]}`, no `content`/`embeds` | — |
| E2 | Flags | `.flags(CreateFlags::SUPPRESS_NOTIFICATIONS)`; `SUPPRESS_EMBEDS|SUPPRESS_NOTIFICATIONS` | `flags` 36864; 36868 | — |
| E3 | Interactive owned | owned, a `#[derive(Button)]` Route button with `by_ref` and a select | sent; `custom_id`s resolved; the Ref is stored in the App's `RefStore`; the select has a nonce | — |
| E4 | Limits | owned, 41 components (Strict); a 4001-char text | `Invalid(Limits CV2-03)` / CV2-04; no request | `WebhookError::Invalid` |
| E5 | Files | 1 referenced and 1 unreferenced attachment; a file over 20 MiB | multipart with `files[0]` only and `attachments:[{"id":0,"filename"}]`; ATT-01 `Invalid`, no request | `WebhookError::Invalid` |
| E6 | Response | Discord answers `{"id":"77","channel_id":"5","attachments":[..]}` | `WebhookMessage.id()` 77, `channel_id()` 5, `thread_id()` None | — |
| E7 | Bad response | 200 with `{}`; 500; 404 code 10015 | `Transport`; `Rejected{500,None}`; `Rejected{404,Some(10015)}` | `WebhookError` |
| E8 | Token | `owned(id, "")`, 513 chars, `"a/b"`, `"a b"`, `"tök"`; a 68-char `[A-Za-z0-9._-]` | `InvalidToken` ×5, nothing sent; ok | `InvalidToken` |
| E9 | Detached | `webhooks()` on a `__testing` detached ctx; `send` | `Detached`, no request | `Detached` |
| E10 | Redaction | `format!("{:?}", webhook)`, `{:?}` of a builder, of `WebhookError` | no token substring | — |
| S1 | Static build | `StaticComponents::new().push(StaticContainer::new().push(TextDisplay::new("x")).push(StaticSection::new([TextDisplay::new("y")], LinkButton::new("https://a.b").label("Docs"))).push(StaticActionRow::buttons([LinkButton::new("https://a.b")])))` | `StaticMessage` JSON equals the same tree built with `Container`/`Section`/`ActionRow` | — |
| S2 | Compile WH-02 | `StaticActionRow::buttons([route_button])`; `StaticSection::new([t], route_button)`; `StaticComponents::new().push(ActionRow::select(sel))`; `StaticContainer::new().push(Container::new())`; `foreign.send(V2Message)`; `foreign.edit(id, components)` | E0277 with `[WH-02]` (fixtures for the first, third and fifth; doctests for the rest) | compile error |
| S3 | Runtime WH-02 | `StaticMessage::try_from(v2)` where v2 has a Route button in a Section accessory and a select in a Container row; a Premium button; link-only | `Limits` with 2 violations at their paths; 1 violation; `Ok` | `UiError::Limits` |
| S4 | Raw WH-02 | `RawComponent` `{"type":1,"components":[{"type":2,"style":1,"custom_id":"x"}]}`; `{"type":2,"style":5,"url":..}` nested; `{"type":17,"components":[{"type":3,..}]}`; `{"type":10,"content":"a"}` | WH-02; ok; WH-02; ok | `UiError::Limits` |
| S5 | Foreign execute | foreign, `send(static_msg)` | same wire as E1 (`with_components=true`) | — |
| S6 | Into V2 | `owned.send(static_msg)` (via `From<StaticMessage> for V2Message`) | accepted, same body | — |
| T1 | Forum no thread | owned created for `GuildForum`, `send(msg)`; `GuildMedia` | `Invalid(WH-05 at thread_id)`, no request | `WebhookError::Invalid` |
| T2 | Forum thread | `.thread(WebhookThread::Existing(9))`; `.thread(WebhookThread::Create("Daily log".into()))` | query `thread_id=9`; body `thread_name`; `thread_id()` 9 / response `channel_id` | — |
| T3 | Thread name bounds | `Create("")`, 100×`a` ok, 101×`a` | WH-05 `Between{1,100}` / ok / WH-05 | `WebhookError::Invalid` |
| T4 | Name off-forum | `GuildText` + `Create("x")`; `GuildText` + `Existing(9)`; unknown kind + `Create("x")` | WH-05 Describe; sent with `thread_id=9`; sent (no precheck) | `WebhookError::Invalid` |
| T5 | Discord forum errors | 400 `{"code":220001}`; 400 `{"code":220003}`; 400 `{"code":50035}` | `Invalid(WH-05)` ×2; `Rejected{400,Some(50035)}` | `WebhookError` |
| T6 | Edit/delete by id on forum | `edit(id,msg)` without `.thread`; with `.thread(9)`; `delete(id)` without | WH-05; `?with_components=true&thread_id=9`; WH-05 | `WebhookError::Invalid` |
| D1 | Edit wire | `owned.edit(77, msg).flags(EditFlags::SUPPRESS_EMBEDS)` | `PATCH /webhooks/{id}/{tok}/messages/77?with_components=true`, body `flags` 32772, `content:null`, `embeds:[]`, `poll:null`, no `sticker_ids`, `attachments:[]` | — |
| D2 | Handle edit diff | `send` with `a.png` (response lists it as id 500), then `handle.edit(msg using a.png and b.png)` | `attachments:[{"id":"500"},{"id":0,"filename":"b…"}]`, only `files[0]` = b | — |
| D3 | Edit by id files | `edit(77, msg using a.png)` | `a.png` re-uploaded as `files[0]` | — |
| D4 | Delete | `handle.delete()` in a thread; `owned.delete(77)` | `DELETE …/messages/77?thread_id=t`; `DELETE …/messages/77` | — |
| C1 | Create | `create(WebhookChannel::new(5, GuildText), webhook_name!("Noctis log"))`, fake answers `{"id":"900","token":"tok","channel_id":"5"}` | `POST /channels/5/webhooks` with bot token and `{"name":"Noctis log"}`; handle id 900, `channel_kind` GuildText | — |
| C2 | Create bad | answer without `token`; 403 code 50013 | `Transport`; `Rejected{403,Some(50013)}` | `WebhookError` |
| R1 | Rules unit | `validate_static_components` on nested trees (row in Container, Section accessory), the Premium style, `Opaque` | per S3 paths; Opaque ignored | — |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/message/{mod.rs (V2Message L69; create_body L159; edit_body L174), body.rs (MessageBody, Inner, clears), flags.rs (CreateFlags/EditFlags already per endpoint)}`.
- `crates/noctis-ui/src/placement.rs`: the model for the sealed static traits (sealed `Sealed`, `on_unimplemented`, macros L200–260). `Arity`/`SectionTexts`/`RowButtons` are in `arity.rs`.
- `crates/noctis-ui/src/component/layout.rs`: `Section::new` L153, `ActionRow::buttons` L246, `Container` L426 (`push`, `accent`, `spoiler`), and the `From` impls into `TopLevelComponent`/`ContainerComponent` L553+.
- `crates/noctis-ui/src/component/raw.rs`: `Check` L232 (the walk to reuse for the WH-02 raw scan).
- `crates/noctis-ui/src/literal.rs` L20–95 (`literal_newtype!`); `crates/noctis-ui/src/lib.rs` L298 `__macro`/re-exports.
- `crates/noctis-ui/src/limits/lower.rs` (`lower_button` L231, `lower_select` L256): lower the V2 tree, then run `validate_static_components`.
- `crates/noctis-rules/src/message.rs` L206 `validate_webhook_name`, tests L363; `src/limits.rs` L137 (`webhook_name_len`), field test L223; `src/component.rs` (`Component` L96, `ButtonStyle` L304, path helpers of `validate_components` L626); `tests/components.rs` L323.
- `crates/noctis-macros/src/literal.rs` (`TextMacro` consts L25–55, `expand_text`) and `src/lib.rs` L110–145.
- `crates/noctis-core/src/responder.rs`:
  - `Shared` L1286 and `build` L1430;
  - `I18n::detached` L1337;
  - `file_limit` L1583, `body` L1602, `signer` L1620, `ref_target` L1632;
  - `prepare` L1737;
  - `encode` L2420;
  - `ResponderError` L356.
- `crates/noctis-core/src/responder/transport.rs`:
  - `Transport` L29 (add `Webhook`);
  - the `follow_up` route pattern L187;
  - `send` L410 (make it `pub(crate)`);
  - `http_error`, `message_id`, `posted`.
- `crates/noctis-core/src/responder/files.rs`: `KnownFiles`, `Files::plan`/`place`/`finish`, `Payload`, `Body.kept`.
- `crates/noctis-core/src/edge/callback.rs`: `valid_token`.
- `crates/noctis-core/src/raw_http.rs` L150–180: the `RequestBuilder::raw` usage to copy for the edit path.
- Context accessors where `webhooks()` goes:
  - `command.rs` L280–300;
  - `component.rs` `ctx_accessors!` L158;
  - `handler.rs` `EventCtx` L313;
  - `app.rs` L246 (`App::shared()` L390);
  - the `Kv::of(Option<&Arc<AppShared>>)` pattern (`kv.rs` L64).
- `crates/noctis-core/src/lib.rs` L209–281: re-exports. `crates/noctis-core/src/responder/tests.rs` L40–70: `Fixture`/`TestServer` (recording upstream) for the webhook unit tests.
- `crates/noctis/src/lib.rs`:
  - `pub mod http` L1585 is the model for `pub mod webhook`;
  - the literal macro re-exports L1636–1651;
  - prelude L1859–1945;
  - `ui` re-exports L2600;
  - `rules` L2718.
- trybuild: `crates/noctis/tests/ui.rs` (glob `tests/ui/**/*.rs`); model fixtures `tests/ui/att/ATT-06_alt_1025.rs`, `tests/ui/msg/MSG-07_create_ephemeral.rs`.
- rules-catalog: `xtask/rules-pending.toml` L55–78 (the WH entries); `xtask/src/rules_catalog/catalog.rs` L566 (`real_catalog_classification`, so check the tier counts still hold after the WH-02 cell change).
- Testkit: `crates/noctis-testkit/src/fake.rs` `route`/`route_with` L655–690 (registered routes win over the TestApp `Wire` fallback); `RecordedRequest.query`/`json()`/`files()`.
- Template: `examples/project-template/src/modules/setup/{handlers.rs (FinishSetup L126), view.rs (render_setup_done L116), mod.rs (GuildConfig, config_key, PickLogChannel channel_types = [GuildText])}`, `locales/{en-US,tr}.yaml` `setup:` L51, `src/strings.rs`, `tests/flows.rs` L43; the skill under `.claude/skills/noctis/` (`responder.md` §7 L746, `SKILL.md` L23/L364, `components-v2.md` L960, `limits-cheatsheet.md` L98/L283–306).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/{message.rs,limits.rs,lib.rs}`: the extended WH-04 check, `webhook_thread_name_len`, `WebhookThreadDecl`, `validate_webhook_thread` and `validate_static_components`. Unit tests `wh_04_reserved_words`, `wh_05_*` (T1, T3, T4) and `wh_02_*` (R1); extend `tests/components.rs`.
- `crates/noctis-ui/src/{message/static_msg.rs (new), message/mod.rs, message/body.rs, literal.rs, component/raw.rs, lib.rs}`: the static builders and traits, `StaticMessage` (+ `TryFrom`), `IntoStaticMessage`, `WebhookName` and `webhook_edit_body`. Unit and doc tests N1, N2, S1, S3, S4 and D1 (body only). `compile_fail` doctests for the S2 cases that have no fixture and for WH-03 (`// [WH-03]`).
- `crates/noctis-macros/src/{literal.rs,lib.rs}`: `webhook_name!`.
- `crates/noctis-core/src/{webhook.rs (new), webhook/tests.rs (new), responder.rs, responder/transport.rs, command.rs, component.rs, handler.rs, app.rs, lib.rs}`: everything in Boundaries, plus the WH-01 `compile_fail` doctest on `ExecuteWebhook` (`// [WH-01]`). Tests over `Fixture`/`TestServer`: E1–E10, S5, S6, T1–T6, D1–D4, C1, C2 (names `wh_01_*`, `wh_02_*`, `wh_03_*`, `wh_05_*`, `msg_07_*` where they pin a rule).
- `crates/noctis/src/lib.rs`: `pub mod webhook`, the `ui`/`rules` re-exports, `webhook_name`, the prelude, module docs with an owned and a foreign example.
- `crates/noctis/tests/ui/wh/` (new): `WH-02_static_row_route_button.rs`, `WH-02_static_select_row.rs`, `WH-02_foreign_send_v2_message.rs`, `WH-04_webhook_name_81.rs`, `WH-04_webhook_name_empty.rs` and `WH-04_webhook_name_reserved.rs`, with blessed `.stderr` containing the rule id.
- `crates/noctis/tests/webhook.rs` (new): one end-to-end test through `noctis::testkit::TestApp`. A slash command that sends a `StaticMessage` through `ctx.webhooks().foreign(..)` and an owned `View` with a Route button, with `http_fake()` routes; assert the recorded queries and bodies (E1, S5).
- `docs/discord-rules.md`, `xtask/rules-pending.toml`, the ARCHITECTURE-SPINE (AD-11 amendment), `xtask/src/skill_check/components.rs`: as in Boundaries.
- `examples/project-template/`:
  - `src/modules/setup/mod.rs`: add `StoredWebhook { id: Id<WebhookMarker>, token: String }` (serde) and `log_webhook_key(guild)`.
  - `view.rs`: add `render_setup_log(config, by, &s) -> StaticMessage` (a Container with one Text Display).
  - `handlers.rs` `FinishSetup`: after `ctx.update(..)`, reuse the stored webhook (`owned(..)?.in_channel(GuildText)`) or `create(WebhookChannel::new(log_channel, ChannelType::GuildText), webhook_name!("Noctis log"))` and store it. Send the log and return the update's `Responded`. A webhook failure is a `tracing::warn!` and never fails setup.
  - Add a `setup.log_enabled` string in `locales/en-US.yaml` and `tr.yaml` and in `strings.rs`.
  - `tests/flows.rs`: add a test with `http_fake()` routes for create and execute; assert `with_components=true`, `flags` 32768, no `content`, and the stored webhook reused on a second Finish.
  - Update `AGENTS.md` and the skill files listed in Boundaries.
- `xtask/public-api.txt`: re-bless it, then update the `SKILL.md` API hash.

**Acceptance Criteria:**
- Given an owned webhook created with `ctx.webhooks().create(..)`, when a View is sent and then edited through its `WebhookMessage`, then:
  - the execute carries `?wait=true&with_components=true` and `flags` with only `IS_COMPONENTS_V2` plus the `CreateFlags` given;
  - the edit is a PATCH with `?with_components=true`, `content: null`, `embeds: []` and `flags` from `EditFlags`;
  - neither request has an `Authorization` header (WH-01, WH-03, MSG-07).
- Given `ForeignWebhook::send`, when it is passed a `V2Message`, or a `StaticActionRow` holding a Route button, then compilation fails with `[WH-02]` (trybuild); and when `StaticMessage::try_from` gets a V2 message with a button or select, then it returns `UiError::Limits` naming WH-02 and nothing is sent.
- Given a webhook whose channel kind is `GuildForum`, when `send` has no `thread(..)`, then `WebhookError::Invalid` names WH-05 and the fake records no request; given `webhook_name!("")`, then compilation fails naming WH-04.
- Given the template's setup Finish, when it runs through TestApp with fake create and execute routes, then a `StaticMessage` log reaches the webhook with `with_components=true`; without those routes, setup still completes.
- Given the finished story, when the local gates run, then `rules-catalog` shows no WH entry pending, and `api-snapshot` and `skill-check` pass with the three new component entries and a matching API hash.

## Spec Change Log

## Review Triage Log

### 2026-10-22 — Review pass
- verdicts: 37 findings — high 0, medium 9, low 15, false 13, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) Webhook token reaches tracing through twilight-http's `req` span — twilight-http 0.17.1 `client/mod.rs` L3000 opens `info_span!("req", url = url)` with the token path and emits INFO/WARN events inside it; the template's default `info` filter prints it. Patch: narrow the "never logged" claim, document the span, add `twilight_http=warn` to the template's default filter.
  - `[medium]` `[patch]` (blind) Template's stored webhook ignores a changed log channel and a deleted webhook — `log_webhook` reuses `logwebhook:{guild}` unconditionally; `StoredWebhook` has no channel id. Patch: store and compare the channel, recreate on 404.
  - `[low]` `[patch]` (blind) No webhook delete, 15-per-channel cap undocumented — the delete-webhook endpoint is excluded by the intent's Never list; the cap (30007) is real and undocumented. Patch: one doc sentence on `create` and in responder.md §7.
  - `[low]` `[patch]` (blind) WH-03 Limit cell still says `files[n]` → 400 on V2 execute — `docs/discord-rules.md` L275 contradicts the uploads `files_upload_and_budget` pins. Patch: reword the cell (grouped with intent D6).
  - `[low]` `[patch]` (blind) `Webhooks::owned` takes the caller's word; WH-02 refusal untyped — ownership is a declaration by design (spec Boundaries; GET webhook excluded), but the rustdoc does not say so. Patch: document it in `owned`'s rustdoc.
  - `[false]` `[reject]` (blind) `create` does not check the channel kind — a thread/DM/category makes Discord refuse Create Webhook, which comes back loudly as typed `WebhookError::Rejected`; the returned `channel_id` is the requested one.
  - `[false]` `[reject]` (blind) `Transport::Webhook` arms pretend success — the spec settles them as Detached-like; `Shared::webhook` is only used by `webhook.rs`, which calls `prepare`/`body` and never an interaction method.
  - `[false]` `[reject]` (blind) `From<ResponderError>` catch-all loses information — spec mandates it; the unlisted variants (`EphemeralUnsupported`, `TokenExpired`, `FollowupCap`, `NoReply`, `ModalUnavailable`, `UpdateUnavailable`, `AlreadyResponded`, `AckDeadlineMissed`) are interaction-only and unreachable from webhook paths; there is no timeout variant.
  - `[low]` `[reject]` (blind) `avatar_url` unchecked — a bad URL gets Discord's typed `Rejected{400,..}`; rare, and a URL check adds a new guard beyond a direct correction.
  - `[false]` `[reject]` (blind) Missing test: template failure path — `setup_flow_validates_then_finishes` runs Finish with no fake webhook routes and asserts "Setup complete"; `WebhookError`'s messages carry no token.
  - `[medium]` `[patch]` (blind) Missing test: forum `Create` handle edit/delete sends the post's channel id as `thread_id` — no handle edit after a thread send is tested. Grouped with vgap "handle edit in thread" and intent D3: add `handle_edit_in_thread`.
  - `[low]` `[patch]` (blind) Missing test: multi-byte `thread_name` at 100 — grouped with vgap "thread_name unknown kind": extend `wh_05_thread_name_bounds`.
  - `[low]` `[patch]` (blind) Missing test: raw button without `style` — `static_walk` flags it today (`!= Some(5)`), but no test pins it. Grouped with vgap "raw types 5-8": extend `wh_02_raw_static_walk`.
  - `[low]` `[patch]` (blind) Docs do not warn that Refs on webhook messages expire — owned sends mint Refs with the App's `ref_ttl`; long-lived log messages outlive them. Patch: a note in module docs and responder.md §7.
  - `[false]` `[reject]` (blind) `Webhook::ready` re-validates the token — the spec orders the token check first before any request; no harm named.
  - `[low]` `[reject]` (edge) Concurrent handle edits race on `KnownFiles` — real but needs two concurrent edits of one message; the fix (an async mutex across the request) adds state and complexity.
  - `[low]` `[reject]` (edge) `avatar_url` unvalidated — same as the blind row: Discord's typed 400, a new guard is more than a direct correction.
  - `[medium]` `[patch]` (edge) Stored webhook reused after the log channel changes — grouped with the blind template row.
  - `[medium]` `[patch]` (edge) Stored webhook deleted in Discord (10015) fails forever — grouped with the blind template row.
  - `[low]` `[reject]` (edge) Concurrent Finish clicks or a failed `put` leak a duplicate webhook — needs a double submit or a KV failure; the fix (compare-and-set, cleanup through an out-of-scope delete endpoint) is more than a direct correction.
  - `[false]` `[reject]` (intent D1) FR-65's "dynamic attempt is a typed error" met only by `try_from` — the epics AC places it under `ForeignWebhook::send`, which accepts only `StaticMessage`; `try_from` is the only dynamic path into it, as the spec settled.
  - `[false]` `[reject]` (intent D2) Ownership declared, not established — spec deliberately settles `owned(id, token)` for stored credentials; verifying needs the GET webhook endpoint the intent excludes (documentation gap patched under the blind `owned` row).
  - `[medium]` `[patch]` (intent D3) AC1's create → send → handle edit wire not tested — grouped into `handle_edit_in_thread` (query, cleared fields, flags, no `Authorization`).
  - `[false]` `[reject]` (intent D4) Click round trip not tested — dispatch is keyed only by the `custom_id`, whose prefixes and Ref storage are asserted; the carrier message does not enter routing.
  - `[false]` `[reject]` (intent D5) Link buttons allowed in `StaticMessage` — spec Design Notes settle Discord's "non-interactive" reading.
  - `[low]` `[patch]` (intent D6) WH-03 cell vs `files[n]` uploads — grouped with the blind WH-03 row.
  - `[false]` `[reject]` (intent D7) WH-05 for unknown kind only after a request — spec Design Notes ("Channel kind") settle this; Discord's 220001/220003 map to the typed error.
  - `[false]` `[reject]` (intent D8) WH-04 widened (clyde/discord, persona) — spec Boundaries settle it as Discord's Create Webhook rule.
  - `[low]` `[reject]` (intent D9) Webhook token not `Zeroizing` (maintainer §5) — §5 sits under the template's bootstrap Memory bullet; noctis-core cannot add `zeroize` (Never: new deps) and keeps the token as a `String`, so wrapping only the template field wipes nothing; more than a direct correction.
  - `[false]` `[reject]` (intent D10) `SUPPRESS_EMBEDS` on Edit Webhook Message unverified — Discord's Edit Webhook Message docs: "The `SUPPRESS_EMBEDS` flag can be both set and unset".
  - `[false]` `[reject]` (intent D11) `skill-check` result not visible in the diff — a gate result, not a code defect; it runs in Verification.
  - `[medium]` `[patch]` (vgap) Webhook locale-key resolution untested — pre-verified. Patch: `webhook_resolves_locale_keys`.
  - `[medium]` `[patch]` (vgap) Handle edit inside a thread never checked for `thread_id` — pre-verified; grouped into `handle_edit_in_thread`.
  - `[medium]` `[patch]` (vgap) Second handle edit never run (`KnownFiles` update unpinned) — pre-verified. Patch: extend `handle_edit_diffs_attachments`.
  - `[low]` `[patch]` (vgap) 220001/220003 mapping untested on edit/delete — pre-verified. Patch: extend `wh_05_discord_forum_errors`.
  - `[low]` `[patch]` (vgap) `thread_name` length never checked for an unknown-kind handle — pre-verified; grouped with the blind multi-byte row.
  - `[low]` `[patch]` (vgap) Raw WH-02 walk tested only with select type 3 — pre-verified; grouped with the blind no-style row.

## Design Notes

- **Static types, not a generic tree.** The erased nodes (`Container`, `Section`, `ActionRow`) can hold anything, so WH-02 cannot be a bound on them without a type parameter on the whole UI kit. Parallel static builders that lower into the same erased nodes give a `[WH-02]` compile error at the push site and reuse every serializer and validator. Run-time trees (`components!`, raw JSON) take the `try_from` path.
- **Premium buttons count as interactive** (conservative: Discord only allows "non-interactive" components through non-owned webhooks). Link buttons are allowed.
- **Channel kind.** The webhook object has no channel type, so WH-05 is checked before sending only when the kind is known: from `create(WebhookChannel)` or from `in_channel(..)` on stored credentials. Discord's codes 220001 and 220003 map to the same typed WH-05 error, so a foreign webhook of unknown kind still gets a typed error.
- **Always clear on edit.** The target's format is unknown without a GET, so every webhook edit nulls `content`, `embeds` and `poll`. That is harmless on a V2 message and required on a legacy one (WH-03). `sticker_ids` is left out because it is not an Edit Webhook Message parameter.
- **No bot token on webhook-token routes,** matching INT-13. Create Webhook uses the bot token (MANAGE_WEBHOOKS).
- **Owned sends reuse the Responder** (`Shared::webhook`), so Route ids, Refs, nonces, key resolution, the Limits and attachments behave exactly as in replies. Locale keys resolve in `en-US` (there is no user or guild locale; Epic 11 may add a per-send locale).
- Example:
  ```rust
  let hook = ctx.webhooks().create(WebhookChannel::new(ch, ChannelType::GuildText), webhook_name!("Mod log")).await?;
  let msg = hook.send(render_case(&case, &s)).username(webhook_name!("Case bot")).await?;
  msg.edit(render_case(&closed, &s)).await?;
  ctx.webhooks().foreign(id, token)?.send(StaticComponents::new().push(TextDisplay::new("Mirrored"))).await?;
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace --all-features` -- all pass (rules, ui, core webhook tests, trybuild `tests/ui/wh/`, doctests)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (no new edges)
- `cargo run --release -p xtask -- rules-catalog` -- ok, WH-01..05 covered
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- matches (install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if missing)
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo test --release --manifest-path examples/project-template/Cargo.toml` -- template flows pass
- `test ! -d target/debug` -- no debug profile

## Auto Run Result

- **Summary:** Views over webhooks (FR-65, WH-01..05): `ctx.webhooks()` with `create(WebhookChannel, webhook_name!)`, `owned(id, token)` and `foreign(id, token)`; owned sends reuse the Responder (Route ids, Refs, nonces, locale keys in en-US, Limits, content-addressed attachments) and return a handle with edit/delete; foreign webhooks accept only `StaticMessage`/`StaticComponents` (compile-time `[WH-02]`, `try_from` for run-time trees); webhook-token routes send no bot token; every edit clears `content`/`embeds`/`poll`; Discord 220001/220003 map to typed WH-05. Template setup module posts to a log channel through a stored webhook.
- **Files changed:** `crates/noctis-core/src/webhook.rs` (+tests) webhook API and transport; `crates/noctis-ui` static builders and raw WH-02 walk; `crates/noctis-macros` `webhook_name!` (WH-04); `crates/noctis/tests/ui/wh/` trybuild compile_fail cases; `crates/noctis/tests/webhook.rs` and `noctis-testkit/tests/http_edge.rs` wire tests; `docs/discord-rules.md` WH rows; `xtask/public-api.txt` re-blessed, `xtask/rules-pending.toml` WH entries removed; `examples/project-template/` setup module, flows tests, AGENTS.md, Usage Skill (responder.md §7, components-v2, limits, testing, troubleshooting).
- **Review findings (37: high 0, medium 9, low 15, false 13):**
  - Patched (9 medium, 12 low, grouped): token in twilight-http `req` span documented and template default filter gets `twilight_http=warn`; template stores the log channel id and recreates the webhook on channel change or 404/10015; 15-per-channel cap documented; WH-03 cell reworded for `files[n]`; `owned` rustdoc says ownership is declared; Ref expiry note on webhook messages; new/extended tests `handle_edit_in_thread`, `webhook_resolves_locale_keys`, second handle edit in `handle_edit_diffs_attachments`, 220001/220003 on edit/delete, multi-byte and unknown-kind `thread_name`, raw button without style and raw types 5-8.
  - Deferred: none.
  - Rejected (low): `avatar_url` unvalidated (Discord's typed 400; new guard beyond a direct correction, x2); concurrent handle edits racing on `KnownFiles` (needs an async mutex across the request); duplicate webhook on double Finish or failed KV `put` (needs compare-and-set plus the out-of-scope delete endpoint); token not `Zeroizing` (no new deps; core keeps a `String`, wrapping only the template wipes nothing).
  - Rejected (false): channel kind on `create` (typed `Rejected`); `Transport::Webhook` arms (only `prepare`/`body` used); `From<ResponderError>` catch-all (spec-mandated, other variants unreachable); template failure-path test (exists); `ready` re-validating token (spec order); FR-65 dynamic path via `try_from` only (as settled); ownership declared (spec Boundaries); click round trip (routing keyed by `custom_id` only); link buttons allowed (spec Design Notes); WH-05 unknown kind after request (Design Notes); WH-04 widened (Discord rule); `SUPPRESS_EMBEDS` on edit (Discord docs allow set/unset); skill-check not in diff (gate result).
- **Follow-up review recommended: true.** Patched counts by verdict: high 0, medium 9, low 12. Named unverified risk: the template's webhook re-creation path (channel change / 404-10015 recovery in `setup/handlers.rs`) and the `twilight_http=warn` filter change were written in the review pass and have only fake-HTTP coverage; a follow-up pass should verify the recreation logic cannot loop or leak webhooks and that no token-bearing span still reaches the template's default logs.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91, template tests, no debug profile). CI paused by maintainer (workflows in `.github/workflows-disabled/`).
- **Residual risks:** concurrent edits of one webhook message can race on `KnownFiles`; double Finish or KV failure can leak a duplicate webhook; webhook token is held as a plain `String`; Refs minted on long-lived webhook messages expire after the App's `ref_ttl`; foreign webhooks of unknown channel kind get WH-05 only after Discord refuses.
