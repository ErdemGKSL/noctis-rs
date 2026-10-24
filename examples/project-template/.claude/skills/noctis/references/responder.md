# Responses and the Responder

Covers: what a handler may return, `IntoResponse`, the context methods
`ctx.respond` / `ctx.update` / `ctx.reply_text`, auto-defer, ephemerality, the
typestate `Responder`, follow-ups, modals, errors shown to users, token expiry,
`LegacyMessage`, webhooks. Rules: INT-01..13, WH-01..05, MSG-05, MSG-07, CV2-01/02.

**Default: build the message with your render function and send it with
`ctx.respond(msg)` (a new message) or `ctx.update(msg)` (update the clicked message),
or a one-line notice with `ctx.reply_text(key)` (§1b).** Returning the message from
the handler still works and sends the same thing (§1). Write no response-type code. Use the typestate Responder only for
modals, follow-ups, or replies that need several messages. A View is just a function
returning a `V2Message` (`views-and-recipes.md`); there is no View trait.

## 1. Return values (`IntoResponse`)

A handler returns `HandlerResult<R>` with `R: IntoResponse<K>` (sealed) for its kind:
`IntoResponse<Command>` (`#[command]`, `#[user_command]`, `#[message_command]`),
`IntoResponse<Component>` (`#[button]` and the select derives) or
`IntoResponse<ModalSubmit>` (`#[modal]`):

| Handler returns | Command | Component / modal submit |
| --- | --- | --- |
| `V2Message` or `Components` (`noctis::ui`) | new message (public, or ephemeral if declared) | `response = update` (default): **updates the clicked message in place** (type 7; a slash-opened modal submit has no message, so it gets a new public message); `response = ephemeral \| public`: a new message (type 4) |
| `LegacyMessage` | plain `content` notice (not for interactive UI, UD-1) | **does not compile** (CV2-01: E0277 `` `LegacyMessage` is not a response of this handler ``, note `[CV2-01] component and modal handlers return a V2Message/Components (or Responded)`): a component's message is never turned back into legacy |
| `()` | **does not compile** (E0277 `` `()` is not a response of this handler ``, pointing at the return type) | same |
| `Responded` (from `ctx.respond`/`ctx.update`/`ctx.reply_text`, or `acked.done()`) | you answered through the context or the Responder; nothing more is sent | same (the auto-defer still fires at the deadline if the handler runs that long without answering) |
| `Err(HandlerError)` | error path (§5) | error path |

Any other type is the same E0277 (`` `u8` is not a response of this handler ``, label:
return a `V2Message`, `Components`, a `LegacyMessage` (commands only), or the `Responded`
from `done()`).

Noctis picks the callback type: a returned message is callback type 4 (or 7 for a
component's `update`) while the interaction is unanswered, and an edit of the original
(`PATCH …/messages/@original`) after the handler or the auto-defer acknowledged it
(INT-12). It adds `IS_COMPONENTS_V2` to every V2 send and edit (CV2-01), never
`EPHEMERAL` on an edit or `update`, sends `allowed_mentions` on every body (the default
`Mentions` ping nobody, MSG-05) and the attachments keep-list `"attachments"` on every
edit body (type-7 `data`, `PATCH @original`, follow-up edits, bot-token edits): files the
edited message already holds are kept by id, new ones uploaded, unused ones dropped
(MSG-08). It runs
the Limits validator (`AppBuilder::limits`, Strict by default; Lenient truncates with
`…`) before any HTTP call. Before encoding, every Route `custom_id` of the message is
resolved: one fresh select nonce per send for every select of the message, and the App's
payload key signs `signed` Routes, and every `by_ref`/spilled Route gets its Ref: the
Responder mints one per distinct Ref slot and stores its record in the App's
`RefStore` with the Route's `ttl` (else `AppBuilder::ref_ttl`) before the request
(`routes-payloads-refs.md` §7). A message with attachments (`V2Message::attachments`)
is sent as `multipart/form-data` (`payload_json` + `files[n]`) with only its used files
(`used_attachments()`), each uploaded once; a re-render that shows the same bytes
uploads nothing. The Upload Budget (ATT-01..03: 20 MiB per file or the interaction's
`attachment_size_limit`, 25 MiB per request, 10 files) is checked before any request;
a miss is `Invalid(UiError::Limits { .. })` in both modes (`components-v2.md` §6).

A returned message that cannot be sent (invalid, rejected) is a handler error: it goes to
the `Command` (or `Component`) Error Sink (§5).

## 1b. `ctx.respond`, `ctx.update`, `ctx.reply_text`

The handler context answers in one call and picks the callback type, deferral, flags
and ephemerality itself, from the declared `response` and what already happened. Each
returns `HandlerResult<Responded>`; return it (or `?` it and answer more).

```rust
#[derive(SlashCommand)]
#[command("dashboard", description = "Open the server dashboard", contexts = [Guild])]
pub struct Dashboard;

impl Dashboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let data = load(&ctx).await?;
        ctx.respond(render_dashboard(&data, &Strings::of(ctx.response_locale()), DashTab::Overview)).await
    }
}

impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if !allowed(&ctx, self.guild) {
            return ctx.reply_text(keys::NOT_FOUND).await;          // ephemeral one-liner
        }
        let data = load(&ctx).await?;
        ctx.update(render_dashboard(&data, &Strings::of(ctx.response_locale()), self.tab)).await
    }
}
```

| Method | On | Argument |
| --- | --- | --- |
| `ctx.respond(msg)` | `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx` | `impl Into<OutgoingMessage>`: a `V2Message`, `Components` or `LegacyMessage` |
| `ctx.update(msg)` | `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx` (**not** `CommandCtx`: E0599, INT-04) | `impl IntoEdit<K>`: a `V2Message` or `Components` (a `LegacyMessage` is E0277, CV2-01) |
| `ctx.reply_text(text)` | every interactive context | `impl Into<NoticeText>`: a `String`, a `&'static str` or a `MessageKey`; returns a `ReplyText` you `.await` (`.public()` first for everyone) |

What each sends, by the interaction's state:

| State when called | `ctx.respond(msg)` | `ctx.update(msg)` | `ctx.reply_text(text)` |
| --- | --- | --- | --- |
| unanswered | type 4, the declared ephemerality (`public` under `response = update`); never type 7 | type 7 on the clicked message (V2 flag, never `EPHEMERAL`) | type 4, **ephemeral** (public with `.public()`) |
| after a type-5 defer (a Command's auto-defer, `response = ephemeral \| public`, or your own `defer_reply`), loading message not edited yet | `PATCH @original`: completes the loading message, keeping its ephemerality (INT-10, INT-12) | `PATCH @original` (the deferred reply, like the typestate `update`) | same visibility: `PATCH @original`; other visibility: the loading message is deleted, then a follow-up |
| after a type-6 defer (a component's auto-defer under `update`) | a follow-up, declared visibility; the clicked message is untouched | `PATCH @original` = the clicked message | a follow-up of the chosen visibility |
| answered already (any reply, update or edit of the original) | another follow-up, declared visibility | `ResponderError::AlreadyResponded`, nothing sent | a follow-up of the chosen visibility |
| after a modal (`show_modal`) or a Middleware stop | `AlreadyResponded` | `AlreadyResponded` | `AlreadyResponded` |
| no message to update (a modal opened from a slash command) | type 4 (public under `update`) | `ResponderError::UpdateUnavailable`, nothing sent, even after the auto-defer; it never falls back to a new message | as the rows above |

- Every send runs the Limits validator first (Strict: `ResponderError::Invalid`, no
  request; Lenient: text truncated with `…`), adds `IS_COMPONENTS_V2` to V2 bodies,
  mints Refs and the select nonce, and sends `allowed_mentions: {"parse": []}`: a
  mention in the text pings nobody (MSG-05). Follow-ups count against the 5-follow-up
  cap of user-installed apps (`FollowupCap`) and fail with `TokenExpired` after 15
  minutes.
- **One initial response per handler (INT-03).** Once the handler answered through a
  context method or a typestate Pending method (`reply`, `defer_reply`, `update`,
  `defer_update`, `show_modal`, …), a second initial response through either API
  (`ctx.update`, or any Pending method of a Responder) is
  `ResponderError::AlreadyResponded` and sends nothing. `ctx.respond` and
  `ctx.reply_text` are never refused for that: they follow up. The auto-defer alone
  never counts as your answer.
- **Keep `response = update` on handlers that call `ctx.update`.** Under `response =
  ephemeral | public` the auto-defer is type 5 (a new loading message), so an
  `update` that loses the race edits that loading message, not the clicked one.
- **A slash-opened modal submit** has no message: answer it with `ctx.respond(msg)`.
  A modal opened both from a button and from a slash command can try `ctx.update`
  and fall back on `UpdateUnavailable` (nothing was sent):

  ```rust
  let card = render_profile(&data, &s, true);
  match ctx.update(card.clone()).await {
      Err(err) if matches!(err.as_error().downcast_ref(), Some(ResponderError::UpdateUnavailable)) => {
          ctx.respond(card).await
      }
      answered => answered,
  }
  ```
- **`reply_text` text.** A `MessageKey` renders in the User Locale first for an
  ephemeral notice and in the Guild Locale first for a public one (`en-US` when
  neither is known or the code does not parse); a key no catalog has renders
  `noctis.error.generic` with one `warn` on `noctis::i18n`. The text is sent as is:
  an empty or blank text is not checked by Noctis (Discord's answer surfaces as
  `Rejected`), and nothing is escaped (use `escape_markdown` on user text yourself).
  `NoticeText` is the conversion target, `ReplyText` the awaitable builder (its
  `Debug` hides the text).
- **Over the Dispatcher** (a prefix Command): `respond` replies to the invoking
  message (an ephemeral declared response is `EphemeralUnsupported`), and
  `reply_text` is always public.
- **Errors** come back as a `HandlerError` wrapping the `ResponderError`: find it with
  `err.as_error().downcast_ref::<ResponderError>()`. Propagate them with `?`; the
  user gets the usual error notice (§5).
- **`response_locale()`** (a resolved `noctis::rules::Locale`) exists on every
  interactive context. On component and modal contexts it follows who sees the
  response: the User Locale for `response = ephemeral` or when `update`-ing an
  ephemeral message, the Guild Locale for a public response (or no source message).
  Keys in a send (`Text::Key`, `ctx.reply_text(key)`) resolve the same way, read when
  the send happens, so a before-Middleware override (`i18n.md` §7) reaches them.
- **`limits()`** (`&Limits`) exists on `CommandCtx`, `ComponentCtx` and
  `ModalSubmitCtx`: the App's Limits (`AppBuilder::limits`), the same ones `respond` and
  `update` validate against. Pass it to a render function that sizes a page with
  `Budget` (`views-and-recipes.md` §4).

The doctrine lint (`xtask skill-check`) accepts these three calls in handlers and
rejects `.reply(`/`.update(`/`.defer_reply(`/`.defer_update(`/`.edit_reply(` on any
other receiver.

## 2. Auto-defer (FR-21, INT-01)

If your handler hasn't answered `ack_deadline` after the gateway received the
interaction (queueing time counts), Noctis defers it for you:

- set it with `App::builder().ack_deadline(Duration::from_millis(1500))`: 0 ms..=2500 ms,
  default 2000 ms (`App::ack_deadline()` reads it); out of range is
  `BuildViolation::InvalidAckDeadline` at `build()`; 0 always defers. There is no
  environment variable for it.
- commands: exactly `{"type":5}`, or `{"type":5,"data":{"flags":64}}` when the Command is
  declared `response = ephemeral` (INT-08); the V2 flag goes on the later edit;
- components and component-opened modal submits: exactly `{"type":6}` (no loading
  state, no `data`) with the default `response = update`; `{"type":5}` /
  `{"type":5,"data":{"flags":64}}` with `response = public` / `ephemeral`;
- a modal submit whose modal a slash command opened has no message to update, so under
  `update` it defers `{"type":5}` (public).

The deadline is App-wide; whether a handler is auto-deferred at all is per handler
(`auto_defer`, §2b).

One gate serializes every initial response, so at most one callback is ever sent
(INT-03): whichever of your handler and the timer gets there first answers. After the
auto-defer, a returned message, `update(msg)` and (after a type-5 defer) `reply(msg)`
become an edit of the original (`PATCH …/messages/@original`; the call still returns
`Ok(Acked)`); after a type-6 defer `reply(msg)` is a **follow-up** (a new message, never
an edit of the clicked panel); a late `defer_reply(..)` or `defer_update()` sends
nothing. A handler that answers after 5 s still delivers, without
"interaction failed". The timer stops when your handler returns, fails, panics or is
cancelled.

Handlers declared `auto_defer = false` are never auto-deferred (a modal must be the
first response, INT-05); if one has not answered within the 3 s deadline (2.9 s), Noctis
logs one `error` on target `noctis::dispatch` naming `handler` and `module`.

## 2b. `auto_defer` and `response`: pick per handler

Every interaction handler declares two things on its own attribute, both checked at
compile time:

- `auto_defer = true | false` (default `true`): whether Noctis defers it at the
  deadline. Allowed on `#[command]`, `#[user_command]`, `#[message_command]` and on a
  component's kind attribute (`#[button(..)]`, `#[string_select(..)]`,
  `#[user_select(..)]`, `#[role_select(..)]`, `#[mentionable_select(..)]`,
  `#[channel_select(..)]`) and on `#[modal(..)]` (a modal submit). `#[handler]` itself
  takes no arguments.
  Never on `#[autocomplete]` (no defer exists for it: `[INT-06]` compile error).
- `response = …`: what the (auto-)defer and the default reply look like. Commands and
  context menus: `public` (default) or `ephemeral`. Components and component-opened
  modal submits: `update` (default, edits the message in place), `ephemeral` or
  `public` (a new message), declared on `#[modal(..)]` for a submit. A modal opened from
  a slash command has no message to update, so its submit may declare `response =
  public` or `ephemeral`; under the default `update` it auto-defers as type 5 public, a
  returned message is a new public message (type 4), and both `update(..)` and
  `ctx.update(..)` return `UpdateUnavailable` without sending anything: answer with
  `ctx.respond(msg)`, `reply(msg)` or a returned message.

`response` is independent of `auto_defer`: with `auto_defer = false` it still sets what
`reply(msg)` and `defer_reply()` use; `reply_as(msg, e)` and `defer_reply_as(e)`
override it for one call.

```rust
#[derive(SlashCommand)]
#[command("settings", description = "Server settings", response = ephemeral)]
pub struct Settings;                         // auto-defer: type 5 + EPHEMERAL

#[derive(Button)]
#[button("mod.ban", response = ephemeral, auto_defer = true, defaults(style = Danger, label = keys::MOD_BAN))]
pub struct Ban { pub user: Id<UserMarker> }  // a click opens a new private panel

#[derive(Button)]                            // a button that opens a modal
#[button("profile.bio", auto_defer = false, defaults(style = Primary, label = keys::PROFILE_EDIT_BIO))]
pub struct OpenBioModal { pub user: Id<UserMarker> }

impl OpenBioModal {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(BioForm::blank(self.user)).await?.done())   // inputs empty
    }
}
```

| The handler … | Declare | Take |
| --- | --- | --- |
| answers with a message, maybe slowly (most handlers) | nothing (`auto_defer` defaults to `true`) | no responder, or `CommandResponder` / `ComponentResponder` / `ModalResponder` |
| opens a modal (a Command or a button) | `auto_defer = false` | `CommandResponder<Manual>` / `ComponentResponder<Manual>`, then `show_modal(m)` within 3 s |
| must control the ack itself (rare) | `auto_defer = false` | the `Manual` responder; call `defer_reply()` / `defer_update()` yourself |
| answers privately | `response = ephemeral` | — |
| opens a new public message from a button | `response = public` | — |
| is a modal submit | nothing (`update` + auto-defer), or `#[modal(.., response = .., auto_defer = ..)]` | `ModalResponder` (`ModalResponder<Manual>` with `auto_defer = false`); never `show_modal` |
| is autocomplete | neither (`[INT-06]` compile error) | nothing: return `Suggestions<T>` |

Compile-time checks (`[INT-05]`): the `#[handler]` method must take the
`…Responder<Manual>` alias exactly when its struct declares `auto_defer = false` (one without the other, or
`auto_defer = false` with no responder parameter, does not compile), and `show_modal` on
an auto-defer responder fails with `[INT-05] show_modal needs auto_defer = false on this
handler`.

Anti-patterns:

- **A button that opens a modal must be `auto_defer = false`.** With the default the
  responder has no `show_modal`, and at runtime the auto-defer would make a modal
  impossible anyway.
- Never declare `auto_defer = false` to "be faster": a handler that misses 3 s shows
  "This interaction failed". Keep the default unless you open a modal.
- Never do slow work before `show_modal`: open the modal first, work after the submit.
- Never pick ephemerality per call to work around a missing declaration: declare
  `response = ephemeral`; use `reply_as`/`defer_reply_as` only for the rare mixed case.

`auto_defer` and `response` ship on `#[command]`, `#[user_command]`,
`#[message_command]`, every component kind attribute and `#[modal]` (carried by the
derive's `__Modal::RESPONSE` and `__Kind::Defer`); only `rate_limit(..)` is still
``[CID-03] `rate_limit` is a runtime argument of #[..] that this Noctis release does not
support yet``.
`CommandHandler::auto_defer()` and `ContextMenuHandler::auto_defer()` read the flag,
`ephemerality()` the declared `response`. Argument errors (the same on `#[modal]`, whose
`auto_defer` help names `ModalResponder<Manual>`): `response = update` on a
command is ``[INT-04] a Command cannot `update` its message: write `response = public`
or `response = ephemeral` ``; any other value is `` `response` takes `public` or
`ephemeral` ``; on a component, `response` takes a bare `update`, `ephemeral` or
`public` (``[CID-03] `response` takes `update`, `ephemeral` or `public` ``, also for
`response = "update"`); `auto_defer` takes only a bool literal (`` `auto_defer` takes
`true` or `false` ``); either given twice is ``[CID-03] #[button] argument `response` is
given twice``; the withdrawn bare `ephemeral` flag and the old modal flag are unknown
arguments whose message names the replacement (`response = ephemeral`,
`auto_defer = false` with `CommandResponder<Manual>`).

If the auto-defer itself fails (for example the interaction sat in the queue past
2.9 s), it logs `warn` "auto-defer failed" (target `noctis::dispatch`, field `handler`)
and changes nothing; your own reply then gets the typed error
(`ResponderError::AckDeadlineMissed`).

The `Dispatcher` (§4.1) has no auto-defer: a message reply has no deadline.

Metrics (label `app` only): `noctis_autodefer_total` (interactions the auto-defer
acknowledged) and `noctis_ack_latency_seconds` (gateway receipt → initial callback, for
every initial callback: reply, defer, modal or auto-defer).

Rendering images is awaited **inside** the handler (`ctx.render(..).await`), so slow
renders are covered automatically.

## 3. Ephemerality (INT-10)

Declared at registration, never toggled later:

- `#[command("…", response = ephemeral)]` (also on `#[user_command]` /
  `#[message_command]`) — private reply (single-user
  UI: settings, setup flows). `CommandHandler::ephemerality()` / `ctx.ephemerality()` read
  it (`Ephemerality::Ephemeral` / `Ephemerality::Public`).
- Commands default to public (shared content).
- Components default to update-in-place (`response = update`: the message keeps its
  visibility; `reply`/`defer_reply` then default to public); `response = ephemeral` on the
  kind attribute — the component opens a **new** private sub-panel.

`reply` and `defer_reply()` and a returned message use the declared value;
`reply_as(msg, Ephemerality)` and `defer_reply_as(Ephemerality)` take it explicitly;
`follow_up(msg, Visibility)` takes one per follow-up (`Visibility` is an alias of
`Ephemerality`); **no edit takes one** (`edit_reply(msg, Ephemerality::…)` is E0061).
After the auto-defer, `reply`/`reply_as` edit the deferred reply, which keeps the
auto-defer's (declared) ephemerality.

Locale follows visibility: `ctx.response_locale()` gives the resolved User Locale for
ephemeral Commands and the resolved Guild Locale for public ones (resolution, DMs and
overrides: `i18n.md` §7). Pass `Strings::of(ctx.response_locale())` to your render
function.

## 3b. Interaction-response flows at a glance (discord.js names)

Each method is its own flow, not an alias. Names follow discord.js.

| Method | discord.js | Callback / route | Effect | Use when |
| --- | --- | --- | --- | --- |
| `reply(msg)` / `reply_as(msg, eph)` | `reply` | type 4 | sends a **new** message (the declared `response`, or `eph`) | answering a command, or a sub-panel off a component |
| `defer_reply()` / `defer_reply_as(eph)` | `deferReply` | type 5 | new "is thinking…" message, filled later by `edit_reply` | the new message needs > 2 s of work |
| `update(msg)` | `update` | type 7 | edits **the clicked component's message** in one request | every button/select that re-renders its own screen (the default) |
| `defer_update()` | `deferUpdate` | type 6 | nothing visible; stops the click spinner | the re-render needs > 2 s; then `edit_reply` |
| `show_modal(m)` | `showModal` | type 9 | opens a modal | `auto_defer = false` handlers only (`Manual` responder); first response only: impossible after any ack, including a defer; never from a modal submit |
| `edit_reply(msg)` | `editReply` | `PATCH …/messages/@original` | edits `@original`: the reply after `reply`/`defer_reply`, the component's message after `update`/`defer_update` | after any ack |
| `fetch_reply()` | `fetchReply` | `GET …/messages/@original` | returns the reply as a `noctis::model::channel::Message` | after any ack |
| `delete_reply()` | `deleteReply` | `DELETE …/messages/@original` | deletes the reply | after any ack |
| `follow_up(msg, vis)` | `followUp` | `POST /webhooks/{app}/{token}` | another new message | after any ack |

A modal can't follow a defer, so only handlers declared `auto_defer = false` (taking the `Manual` responder, §2b) have `show_modal`; they get no auto-defer and must call it within 3 s: open the modal first and do slow work after the submit, never before it.

Equivalences (asserted on the wire): `defer_update()` + `edit_reply(msg)` sends `{"type":6}`
then `PATCH …/messages/@original` with exactly the type-7 `data` of `update(msg)`, so it
ends like `update(msg)` but takes two requests; prefer `update` when the render is fast.
`defer_reply_as(e)` + `edit_reply(msg)` ends like `reply_as(msg, e)` (the edit carries no
`EPHEMERAL`; the message keeps the defer's ephemerality). You rarely call `defer_*`
yourself: the auto-defer fires at 2000 ms (type 6 for components under `update`, type 5
otherwise).

### Component and modal-submit flows

| Call (state) | `response = update` on a message | `response = ephemeral \| public` | slash-opened modal submit |
| --- | --- | --- | --- |
| `update(msg)` (Pending) | type 7 with `data` = the edit body of the message's format | type 7 (the clicked message) | `UpdateUnavailable`, no request, still unanswered |
| `defer_update()` (Pending) | `{"type":6}` | `{"type":6}` | `UpdateUnavailable`, no request |
| `reply(msg)` (Pending) | type 4, public | type 4 with the declared ephemerality | type 4, public |
| `defer_reply()` (Pending) | `{"type":5}` | `{"type":5}` / `{"type":5,"data":{"flags":64}}` | `{"type":5}` |
| returned message (Pending) | type 7 | type 4 | type 4, public |
| auto-defer | `{"type":6}` | type 5 (+ `EPHEMERAL`) | `{"type":5}` |
| `update(msg)` after any ack | `PATCH …/@original` | `PATCH …/@original` (the deferred type-5 reply, not the clicked message) | `UpdateUnavailable`, no request (never falls back) |
| returned message after any ack | `PATCH …/@original` | `PATCH …/@original` | `PATCH …/@original` (the deferred type-5 reply) |
| `reply(msg)` after a type-6 ack | public follow-up (`POST /webhooks/{app}/{token}?wait=true`) | follow-up with the declared ephemerality (after an explicit `defer_update()`) | — |
| `reply(msg)` after a type-5 ack | `PATCH …/@original` | `PATCH …/@original` | `PATCH …/@original` |
| `defer_reply()` / `defer_update()` after any ack | nothing (`debug`) | nothing | nothing |
| `show_modal(m)` after any ack | `ModalUnavailable`, no request | same | (no `show_modal` on a modal submit) |
| `fetch_reply()` after `update` / `defer_update` | `GET …/@original`: the clicked message | same | — |
| `delete_reply()` after `update` / `defer_update` | `DELETE …/@original`: deletes the clicked message | same | — |
| `fetch_reply()` / `delete_reply()` after a type-5 ack | the deferred reply's `@original` | same | the deferred reply's `@original` |

After `update`/`defer_update`, `@original` is the clicked message (its id and channel are
known), so `edit_reply`, `fetch_reply` and `delete_reply` act on it; after 15 minutes a
public one still goes through the bot-token channel routes
(`PATCH`/`GET`/`DELETE /channels/{channel}/messages/{id}`, INT-02); on an ephemeral one
all three are `TokenExpired` with no request.

A `reply` after a type-6 ack is a follow-up, not `@original`: the returned Acked
responder's `edit_reply`/`fetch_reply`/`delete_reply` still act on the clicked message,
so that reply can't be edited through it. To edit or delete it later, send it with
`follow_up(msg, vis)` instead and keep its `FollowupHandle`.

Component and modal edits are V2 only (CV2-01, compile time): `update`,
`update_with_response` and `edit_reply` of a `ComponentResponder`/`ModalResponder` take a
`V2Message` or `Components`, and a component or modal handler returns one of them (or
`Responded`). A `LegacyMessage` there is E0277 ``[CV2-01] a component's message can't be
turned back into a legacy message: send a `V2Message` ``. `reply`, `reply_as`,
`follow_up` and follow-up edits still take a `LegacyMessage`: they send a new message. A
legacy source message is updated into V2 (its legacy fields are cleared, CV2-02).

A `signed` modal (`#[modal("id", signed, ..)]`) verifies its tag on submit once a payload
key is set (`NOCTIS_PAYLOAD_KEY`, current or previous key): the handler runs with the
verified Payload fields and the inputs; a bad tag gets the generic notice (CID-06).
Decoding it yourself with `ModalForm::from_json` is always `SubmitError::Malformed`,
since only dispatch knows the key.

### `…_with_response`

`reply_with_response(msg)` and `defer_reply_with_response()` (all kinds),
`update_with_response(msg)` and `defer_update_with_response()` (components and modal
submits) send the callback with `?with_response=true` and return
`(Responder<K, Acked, D>, Option<CallbackResource>)`. `CallbackResource::kind()` is the
callback type Discord answers (4, 5, 6 or 7) and `message()` the
`noctis::model::channel::Message` it created or updated, when it sent one that parses.
The resource is `None` exactly when no callback was sent (the interaction was already
acknowledged, so the call became an edit, a follow-up or nothing); a response body without
`resource` is `ResponderError::Transport`.

```rust
let (acked, resource) = r.update_with_response(panel).await?;
if let Some(message) = resource.as_ref().and_then(|r| r.message()) {
    tracing::debug!(id = %message.id, "panel updated");
}
Ok(acked.done())
```

## 4. The Responder (typestate, INT-03/04)

The type is `Responder<K, S = Pending, D = AutoDefer>`: `K` is the handler kind
(`Command`, `Component` or `ModalSubmit`; sealed `ResponderKind`), `S` the state
(`Pending` or `Acked`, sealed `ResponderState`) and `D` the sealed `DeferMode`:
`AutoDefer` (default) or `Manual` (§2b). Only `Manual` implements the sealed
`CanShowModal`. Take it
as the last handler parameter through its alias, after `ctx` (and the path or target):
`handle(self, ctx[, path][, r: CommandResponder])`, `handle(self, ctx, target[, r:
CommandResponder])` on a context menu, `handle(self, ctx: ComponentCtx[, r:
ComponentResponder])` on a component and `handle(self, ctx: ModalSubmitCtx[, r:
ModalResponder])` on a modal; the options, Payload and inputs are `self`. Any other
order, two responders, or another kind's responder (a `CommandResponder` on a component)
is a compile error listing these forms (`the parameters of this #[handler] method do not
fit ..`). A handler that returns a message takes none (and if it takes
one but never uses it, nothing is sent from it).

| Alias | Pending methods | Kind |
| --- | --- | --- |
| `CommandResponder` (auto-defer) | `reply(msg)`, `reply_as(msg, e)`, `defer_reply()`, `defer_reply_as(e)` — **no** `show_modal` | chat input / context menu |
| `CommandResponder<Manual>` | the above plus `show_modal(m)`; required exactly when the command is `#[command(.., auto_defer = false)]` (`[INT-05]` compile error otherwise) | command that opens a modal |
| `ComponentResponder` (auto-defer) | `reply(msg)`, `reply_as(msg, e)`, `update(msg)`, `defer_update()`, `defer_reply()`, `defer_reply_as(e)` and the `…_with_response` variants — **no** `show_modal` | buttons, selects |
| `ComponentResponder<Manual>` | the above plus `show_modal(m)`; required exactly when the component declares `auto_defer = false` | component that opens a modal |
| `ModalResponder` | `reply(msg)`, `reply_as(msg, e)`, `update(msg)`, `defer_update()`, `defer_reply()`, `defer_reply_as(e)` and the `…_with_response` variants — **never** `show_modal` (E0599, INT-04) | modal submit |
| `ModalResponder<Manual>` | the same, never auto-deferred; required exactly when the modal declares `#[modal(.., auto_defer = false)]` (`[INT-05]` otherwise); still **no** `show_modal` (E0599) | modal submit that answers itself |

Autocomplete has no Responder and no response method: the `#[autocomplete(field)]`
method returns `HandlerResult<Suggestions<T>>` and Noctis sends it (`commands.md` §4).
`AutocompleteCtx` has no `respond`, `defer` or `reply`.

Every Pending method consumes the responder and returns `Responder<K, Acked, D>`:

```rust
r.reply(msg).await?                     -> Responder<Command, Acked>  // callback type 4, declared ephemerality
r.reply_as(msg, Ephemerality::Public).await? -> Responder<Command, Acked>  // per-call override
r.defer_reply().await?                  -> Responder<Command, Acked>  // callback type 5, declared ephemerality; only EPHEMERAL (INT-08)
r.defer_reply_as(Ephemerality::Ephemeral).await? -> Responder<Command, Acked>  // per-call override
r.show_modal(m).await?                  -> Responder<Command, Acked, Manual> // Manual responder only: callback type 9
r.update(msg).await?                    -> Responder<Component, Acked>  // ComponentResponder / ModalResponder: callback type 7
r.defer_update().await?                 -> Responder<Component, Acked>  // callback type 6
r.reply_with_response(msg).await?       -> (Responder<K, Acked>, Option<CallbackResource>)
acked.edit_reply(msg).await?            -> ()
acked.fetch_reply().await?              -> noctis::model::channel::Message
acked.follow_up(msg, Visibility::Public).await? -> FollowupHandle
acked.delete_reply().await?             -> ()
acked.done()                            -> Responded   // return this from the handler
handle.message_id()                     -> Id<MessageMarker>
handle.edit(msg).await?                 -> ()
handle.delete().await?                  -> ()
```

`msg` is anything `Into<OutgoingMessage>`: `V2Message`, `Components` or `LegacyMessage`
([planned] `StaticMessage`); on a component or modal responder, `update`,
`update_with_response` and `edit_reply` take only `V2Message`/`Components` (the sealed
`IntoEdit<K>`; a command's `edit_reply` also takes a `LegacyMessage`, CV2-01). Calling a method that doesn't exist for the kind or
state is a compile error: a second `reply` (E0599), `update` on a `CommandResponder`
(E0599), `show_modal` on a `ModalResponder` (E0599), `show_modal` on an auto-defer
responder (`[INT-05] show_modal needs auto_defer = false on this handler`, E0277),
`defer_reply_as(CallbackFlags::EPHEMERAL)` (E0308). `Responder`,
`Responded` and `FollowupHandle` are `#[must_use]`; `Debug` never prints the token.

Edits keep the format rules: editing a legacy original into a V2 message sends the
legacy clears (CV2-02). On a component or modal responder a `LegacyMessage` edit does not
compile (CV2-01); on a `CommandResponder`, whose original's format is known only at run
time, a V2 original edited with a `LegacyMessage` is
`ResponderError::Invalid(UiError::V2ToLegacy)` without a request.

Every call returns `Result<_, ResponderError>` (`#[non_exhaustive]`; `?` converts it into
`HandlerError`):

| Variant | When (nothing is sent unless noted) |
| --- | --- |
| `Invalid(UiError)` | Limits violation (Strict), an Upload Budget miss (ATT-01..03, both modes) or a command's V2 → legacy edit |
| `Encode { reason }` | a Route `custom_id` cannot be resolved (a `signed` Route without a payload key on a detached responder, a select nonce or Ref that could not be drawn, or a `custom_id` that fails to encode or finish); `reason` names the Route |
| `RefStore(PortError)` | the `RefStore` failed while storing a Ref record of the message |
| `RefCollision` | three freshly minted Refs in a row already existed in the `RefStore` |
| `EphemeralUnsupported` | an ephemeral response over a Dispatcher message reply (§4.1) |
| `InvalidToken` | the interaction token is not 1-512 characters of `[A-Za-z0-9._-]` |
| `AckDeadlineMissed(AckDeadlineMissed)` | the initial response came after 2.9 s (INT-01), or the callback exceeded `.http_timeout(..)` |
| `RateLimited(RateLimited)` | Discord rate limited a follow-up, edit or fetch (RL-04): `scope`, `retry_after`, `discord_code`; never retried, wait before trying again |
| `Rejected { status, code }` | Discord answered an error (`code`: its JSON error code, e.g. `Some(10015)`); a callback answered 40060 (already acknowledged) counts as success |
| `Transport` | no usable response (also a `fetch_reply` body that is not a message) |
| `NoReply` | `fetch_reply()` with no reply message to fetch: a `Dispatcher` handler that has not replied yet (§4.1) |
| `TokenExpired` | 15 min after the interaction arrived: a follow-up, or an edit/delete of an ephemeral message or of one whose id is unknown (INT-02) |
| `FollowupCap` | the 6th follow-up of a user-install-only interaction (INT-11) |
| `ModalUnavailable` | `show_modal(m)` after another initial response (an auto-defer cannot happen for `auto_defer = false`), over the `Dispatcher`, or any dynamic attempt to show a modal on a modal submit (no request is sent) |
| `UpdateUnavailable` | `update`/`defer_update` (or their `…_with_response`) on a submit of a modal that a slash command opened: "there is no message to update: this modal was not opened from a component message"; no request is sent and the interaction stays unanswered, but `update`/`defer_update` consumed the responder: answer by returning a message (a type-4 public reply), or call `reply` instead of `update` when the submit may come from a slash command |

Runtime guards the types can't express:
- if the auto-defer already fired, your `reply` becomes `edit_reply` transparently
  (a follow-up after a component's type-6 defer) and `update` becomes `edit_reply` (§2);
- follow-ups are counted when `authorizing_integration_owners` has a user install and no
  guild install: at most 5, the 6th is `ResponderError::FollowupCap` with no request
  (INT-11). A follow-up that fails validation uses no slot; one Discord rejects still
  uses its slot. Guild installs have no cap;
- 15 minutes after the gateway received the interaction the token is expired (INT-02):
  `edit_reply`, `fetch_reply`, `delete_reply`, `handle.edit` and `handle.delete` on a
  **public** message whose id is known fall back to the bot-token channel routes
  (`PATCH`/`GET`/`DELETE /channels/{channel}/messages/{id}`; a user-installed app outside the
  guild gets `Rejected`); everything else (follow-ups, ephemeral messages, an original
  that was deferred and never edited) is `ResponderError::TokenExpired` without an HTTP
  request. A returned message past 15 min follows the same rule.

Example: reply privately, then post a public log line.

```rust
#[derive(SlashCommand)]
#[command("mod warn", description = "Warn a member", permissions = MODERATE_MEMBERS, response = ephemeral)]
pub struct Warn { #[option(description = "Who")] pub user: ResolvedUser }

impl Warn {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let case = record_warning(&ctx, &self).await?;
        let acked = r.reply(warn_result(&case)).await?;           // V2Message or Components
        acked.follow_up(case_log(&case), Visibility::Public).await?;
        Ok(acked.done())
    }
}
```

Example: defer, work, then return the message (it edits the deferred original):

```rust
#[derive(SlashCommand)]
#[command("report", description = "Build the weekly report")]
pub struct Report;

impl Report {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Components> {
        let _ = r.defer_reply().await?.done();
        Ok(build_report(&ctx).await?)
    }
}
```

Example: a Command that opens a modal (INT-05). After a modal there is no original
message, so return `done()`:

```rust
#[derive(SlashCommand)]
#[command("bio", description = "Edit your profile bio", auto_defer = false, contexts = [Guild])]
pub struct Bio;

impl Bio {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(BioForm::blank(ctx.user_id())).await?.done())
    }
}
```

`show_modal(m)` takes `impl Into<Modal>`: a runtime `Modal`, a derived modal value or
`X::blank(..)`, or the `ModalBuilder` of `X { .. }.build()` with its `.title(..)`,
`.value(X::FIELD, v)` and `.overrides(..)` (`modals.md` §4a). It resolves locale keys in
the User Locale first, validates the modal against the Limits (`Invalid`: also a
pre-fill over a Text Input's `max_length` or more defaults than `max_values`, in every
Limit mode) and encodes it (`Encode` for an unresolved Route id) before any request.
Every send resolves `MessageKey` texts (labels, placeholders) the same way: User then
Guild Locale for an ephemeral send, Guild then User for a public one, else en-US; a
missing key is sent as the key with one `warn` on `noctis::i18n`.

Example: a component that opens a modal (its struct declares `auto_defer = false`):

```rust
#[derive(Button, Clone, Copy)]
#[button("profile.bio", auto_defer = false, defaults(style = Primary, label = keys::PROFILE_EDIT_BIO))]
pub struct EditBio { pub user: Id<UserMarker> }

impl EditBio {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(BioForm::blank(self.user)).await?.done())
    }
}
```

### 4.1 Running a Command from elsewhere: the `Dispatcher`

`ctx.dispatcher()` on an `EventCtx` gives the App's `Dispatcher` (`Clone`), which runs a
registered Command outside an interaction, for example a prefix command. For `!command`
messages use the shipped `noctis::prefix::PrefixPlugin` (`prefix` feature,
`commands.md` §9), which parses arguments against the Command's options and does all of
the below; write your own caller only for another trigger:

```rust
#[derive(EventHandler)]
#[event]
pub struct Prefix;

impl Prefix {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        if msg.author.bot {
            return Ok(()); // never answer bots (or yourself)
        }
        let Some(text) = msg.content.strip_prefix('!') else { return Ok(()) };
        // `!warn 3`: the first word is the Command Path, the rest its arguments.
        let mut words = text.split_whitespace();
        let Some(path) = words.next() else { return Ok(()) };
        let mut input = CommandInput::new(path);
        if let Some(days) = words.next().and_then(|w| w.parse().ok()) {
            input = input.with_option("days", OptionValue::Integer(days));
        }
        let invocation = CommandInvocation::new(input, msg.channel_id, msg.id, msg.author.id)
            .guild_id(msg.guild_id) // the guild's scope first, then global
            .shard(ctx.shard())     // optional, default 0
            .locale("tr");          // optional User Locale
        ctx.dispatcher().run_command(invocation).await?; // Result<(), DispatchError>
        Ok(())
    }
}
```

- `CommandInput::new(path)` plus `.with_option(name, OptionValue::…)` (a later value of
  the same name replaces the earlier one): you decode your own options; the handler's
  options struct is built and validated from them as usual (a missing required one is
  an `OptionError` in the Command Error Sink). When your own parsing fails, mark the
  input with `.with_error(OptionError::…)`: the handler does not run and the user gets
  the same public `noctis.validation.failed` reply as for a slash decode.
- `run_command` resolves the path in the guild's scope, then globally, and runs the
  handler. An unregistered path is `Err(DispatchError::UnknownCommand { path })` with no
  request; a failing handler goes to the `Command` Error Sink, not into the result.
- The Dispatcher enforces none of the Command's `default_member_permissions`, `contexts`
  or `nsfw` metadata (Discord enforces those only for real interactions): check them
  yourself before calling `run_command`.
- The response is a message transport: `reply` and a returned message post a reply to
  the invoking message (`message_reference`, `fail_if_not_exists: false`), `defer_reply()` (public)
  shows the typing indicator (the later message is then posted as the reply), edits,
  `fetch_reply` (bot token) and deletes target that reply (`fetch_reply` before it is
  `ResponderError::NoReply`), follow-ups are plain channel messages. Anything ephemeral
  (a `response = ephemeral` Command, `reply_as(.., Ephemeral)`, `defer_reply_as(Ephemeral)`, `follow_up(.., Ephemeral)`) is
  `ResponderError::EphemeralUnsupported`, never a silent public post; `show_modal(m)` is
  `ResponderError::ModalUnavailable`. No auto-defer runs.
- Span `noctis.dispatch{app, guild_id, module, path}`.

## 5. Errors shown to users (FR-22)

```rust
// A known, user-facing condition: localized ephemeral notice, logged at debug,
// never sent to an Error Sink.
return Err(HandlerError::user(keys::COMMON_GUILD_ONLY));

// Any error type: `?` converts (impl<E: Error + Send + Sync + 'static> From<E>).
let row = db.load(id).await?;

// An error with a friendly message: the notice uses its key, the error still
// reaches the Error Sink.
db.save(x).await.map_err(|e| HandlerError::new(e).with_message(keys::SAVE_FAILED))?;
```

`HandlerError` API: `new(e)`, `user(key)`, `with_message(key)` (`#[must_use]`; on a
`user(..)` error it replaces the key and it stays a user condition), `user_message() ->
Option<MessageKey>` (`Some(key)` for `user`/`with_message`, else `None`), `as_error()`
(downcast the inner error; `user(key)` wraps a private error whose `Display` is
``user notice `<key>` ``). `HandlerResult<T>` = `Result<T, HandlerError>`.

When a handler (Command, component or modal submit) returns `Err`, panics, or a
Command's options fail to decode or validate, the user gets **one short notice** (a V2
message with one `TextDisplay`), then the error is handled:

| Key of the notice | When |
| --- | --- |
| the error's key | `HandlerError::user(key)` or `.with_message(key)` |
| `noctis.validation.failed` | an option failed to decode (`OptionError`) and no key was given |
| the validator's key | a `validate = path` failed (`InvalidOption`; the handler does not run) |
| `noctis.error.generic` ("Something went wrong. Please try again later.") | any other error, and every panic |

Where it goes depends on what the handler already sent:

| State when the handler failed | Notice |
| --- | --- |
| nothing sent yet | the reply: callback type 4 with `EPHEMERAL` (flags 32832), **even for a public Command** |
| deferred with type 5 (by you or by the auto-defer) | the deferred original is deleted (`DELETE …/messages/@original`; a failure is a `warn` and the notice is still sent), then an ephemeral follow-up (AD-23) |
| a component deferred with type 6, or updated | an ephemeral follow-up; the clicked message is **never** deleted |
| replied | an ephemeral follow-up only |
| opened a modal | nothing (`debug` "no notice after a modal") |
| cancelled at shutdown | nothing; reported as `ReportKind::Cancelled` |
| `Dispatcher` (§4.1) | public (a message has no ephemeral form): a reply to the invoking message while unanswered, else a channel message |

The notice is sent before the auto-defer timer stops (the gate orders the two). Follow-up
cap (INT-11) and token expiry (INT-02) apply: when the notice cannot be sent (cap,
expired token, Discord rejects it) Noctis logs one `warn` "could not send the error
notice" (target `noctis::dispatch`, field `handler`) and never retries; it is not an
Error Sink report.

Then:
- **Errors and panics**: `noctis_handler_errors_total{kind="command"}` (`kind="component"`
  for components and modal submits, which also counts a returned message that could not
  be sent) +1 and a report to the `Command` (or `Component`) Error Sink (`ReportKind::Error` / `Panic`, with `module`, `handler` and
  `event_type = "INTERACTION_CREATE"`). The full error chain is logged once inside the
  `noctis.interaction` span: by the default `LogSink`, or, with a custom `Command` (or
  `Autocomplete`) sink, by `LogSink` right before your sink is called — so a custom sink
  never hides the error from the logs. The shard keeps running (NFR-4).
- **User conditions** (`HandlerError::user(key)` and a failed option validator): one
  `debug` on target `noctis::command` (`noctis::component` for components; fields
  `handler`, `key`); no Error Sink, not
  counted in `noctis_handler_errors_total`. `with_message` does **not** make an error a
  user condition.

The locale of the notice is the User Locale, then the Guild Locale, then `en-US` (an
unparseable code means `en-US`). Keys resolve from the catalogs registered with
`AppBuilder::locales(strings::catalog())` (`i18n.md` §2), and the built-in `noctis.*`
texts are English unless a locale file overrides them (`i18n.md` §6). A key no
registered catalog defines (`keys::SAVE_FAILED` without `.locales(..)`) shows the
generic text and Noctis logs `warn` "missing message key" (target `noctis::i18n`,
field `key`).

Logs never contain the token, a user id, option values or the notice text.
Autocomplete never shows a notice (it answers an empty list, `commands.md` §4); event
handlers have no user to notify (`HandlerError::user` there is reported like any other
error). Route errors by origin with Error Sinks (`modules-middleware-ratelimits.md`).
A click or modal submit that reaches no handler (malformed, unknown Route, kind mismatch,
forged, undecodable, invalid submission, no author) gets the same generic ephemeral
notice (`noctis.error.generic`, type 4 with flags 32832) after its `warn` (CID-06,
`routes-payloads-refs.md` §1). Stale and Ref ids go to the Expired Handler chain instead
(`#[on_stale]`, `#[on_expired]`, `.expired_handler(f)`, else the `noctis.expired` notice;
`routes-payloads-refs.md` §8).
A Route Guard denial (`routes-payloads-refs.md` §1a) uses the same routing: one
ephemeral type-4 notice while pending; after an auto-defer an ephemeral follow-up, with
a type-5 deferred original deleted first.
[planned, Epic 13] Notices from the "disabled here" and rate-limit stages use the same
routing.

## 6. `LegacyMessage`

`LegacyMessage::new("…")` for plain notices or prefix commands; the 2000-character limit
(MSG-01) is checked by the runtime validator (`noctis::ui::validate`) and, for a
literal, at `cargo check` by `content!("…")`; see `components-v2.md` §12. Up to 5 legacy Action Rows (CV2-15). Never for
dashboards. Editing a legacy message into a View is supported (content/embeds/poll/sticker_ids
are cleared in the same request);
V2 → legacy is a typed error (CV2-01).

## 7. Webhooks (FR-65)

Every context (`ctx.webhooks()` on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx`, `EventCtx`)
and the `App` (`app.webhooks()`) hand out a `Webhooks` factory. It gives two kinds of
handle, told apart by type (`noctis::webhook`):

| Handle | Made by | Sends |
| --- | --- | --- |
| `OwnedWebhook` (`Webhook<Owned>`) | `create(channel, name)` (the App creates it), or `owned(id, token)` for credentials you stored after a `create` | any `V2Message`: Route buttons, selects, Refs and nonces resolve exactly as in a reply |
| `ForeignWebhook` (`Webhook<Foreign>`) | `foreign(id, token)` for a webhook a user or another app made | only a `StaticMessage` / `StaticComponents` (`components-v2.md` §12b); a `V2Message` is a `[WH-02]` compile error |

```rust
use noctis::prelude::*;
use noctis::twilight::model::channel::ChannelType;
use noctis::webhook::{WebhookChannel, WebhookError, WebhookThread};

// Create once (the bot needs MANAGE_WEBHOOKS), store id + token + channel like a password.
let hook = ctx
    .webhooks()
    .create(WebhookChannel::new(log_channel, ChannelType::GuildText), webhook_name!("Audit log"))
    .await?;
store(hook.id(), hook.token(), log_channel);

// Later: reuse the stored webhook only while it is the configured channel's (else
// create and overwrite); a 404 means it was deleted in Discord: forget it, create a new
// one and send once more (the template's `setup/handlers.rs` `post_log` does this).
let hook = ctx.webhooks().owned(id, token)?.in_channel(ChannelType::GuildText);
let sent = hook
    .send(render_log(&entry, &s))                  // V2Message from your render fn
    .username(webhook_name!("Case bot"))           // per-message persona (WH-04)
    .avatar_url("https://example.com/avatar.png")
    .await;                                        // WebhookMessage<Owned>
let posted = match sent {
    Err(WebhookError::Rejected { status: 404, .. }) => {
        forget(); // delete the stored credentials
        let hook = ctx
            .webhooks()
            .create(WebhookChannel::new(log_channel, ChannelType::GuildText), webhook_name!("Audit log"))
            .await?;
        store(hook.id(), hook.token(), log_channel);
        hook.send(render_log(&entry, &s)).await?
    }
    sent => sent?,
};
posted.edit(render_log(&closed, &s)).await?;       // keeps its attachments
posted.delete().await?;

// A webhook you don't own: static content only.
let mirror = ctx.webhooks().foreign(other_id, other_token)?;
mirror.send(render_notice(&s)).await?;             // render_notice -> StaticMessage
```

- **Handles and requests.** `Webhook<O>` (`O: Ownership`, sealed: `Owned` or `Foreign`)
  has `.id()`, `.token()`, `.channel_kind()` (`Some` after `create` or `.in_channel(kind)`),
  `.send(msg)` → `ExecuteWebhook` (`.flags(..)`, `.username(..)`, `.avatar_url(..)`,
  `.thread(..)`), `.edit(message_id, msg)` → `EditWebhookMessage` (`.flags(EditFlags)`,
  `.thread(thread_id)`) and `.delete(message_id)` → `DeleteWebhookMessage` (`.thread(..)`).
  Each request does nothing until `.await`ed. A send returns `WebhookMessage<O>`:
  `.id()`, `.channel_id()`, `.thread_id()`, `.edit(msg)`, `.delete()`.
- **Always V2, always `with_components=true`** (WH-01): every execute sends
  `?wait=true&with_components=true` and every edit `?with_components=true`; there is no
  setter to turn it off. Webhook-token requests never carry the bot `Authorization`
  header.
- **Flags per endpoint** (MSG-07): `.flags(CreateFlags)` on a send, `.flags(EditFlags)` on
  an edit. There is no ephemeral webhook message.
- **Edits clear legacy fields** (WH-03): every edit sends `content: null`, `embeds: []` and
  `poll: null` with `flags = 32768 | your bits`, so a legacy webhook
  message becomes V2. `hook.edit(message_id, msg)` (by id) re-uploads every attachment
  `msg` uses; `posted.edit(msg)` on the returned `WebhookMessage` keeps the files Discord
  already holds.
- **Threads, forum and media channels** (WH-05): `.thread(thread_id)` (a `WebhookThread::Existing`) posts into an
  existing thread; `.thread(WebhookThread::Create("Weekly report".into()))` opens a new
  forum or media post (name 1–100). A forum or media channel without a thread, or a thread
  name in a text channel, is `WebhookError::Invalid` before anything is sent when the
  channel kind is known (`create` knows it; `owned(..)`/`foreign(..)` learn it from
  `.in_channel(kind)`). Without a known kind, Discord's own answers 220001/220003 map to the
  same WH-05 error. `WebhookMessage::thread_id()` is the thread to pass to a by-id
  `.edit(..).thread(t)` / `.delete(..).thread(t)` later.
- **Names** (WH-04): `WebhookName` is 1–80 characters with no `clyde` or `discord` (any
  case). `webhook_name!("…")` checks a literal at `cargo check`
  (`[WH-04] …`); `WebhookName::try_new(text, &limits)` / `WebhookName::try_from(text)` at
  run time.
- **Validation.** A send or edit runs the Responder's preparation: locale keys, Route
  ids, Refs and nonces, `Limits`, the Upload Budget, attachments.
- **Tokens are secrets.** `Webhook`'s `Debug` prints no token, errors never contain it,
  and Noctis never logs it. twilight-http's `req` span carries the request URL, which
  for a webhook-token request contains the token, so keep `twilight_http=warn` in your
  `RUST_LOG` filter (the template's default does). Keep it out of your own logs and
  messages.
- **Store and reuse a created webhook.** A channel holds at most 15 webhooks (beyond:
  `Rejected { status: 400, code: Some(30007) }`), and lost credentials leak a webhook
  Noctis cannot delete, so store the created webhook's credentials and reuse them.
- **Ownership is your declaration.** Noctis does not verify that `owned(..)` credentials
  belong to the App; a webhook the App did not create goes in `foreign(..)`. Discord
  refuses interactive components through a non-owned webhook (`WebhookError::Rejected`).
- **Refs expire.** Route buttons on an owned webhook message mint Refs with the App's
  `ref_ttl`, so a `by_ref` button on a long-lived log or persona message expires into
  `on_expired`. Prefer inline (non-`by_ref`) Routes there, or a longer `ttl`.

**Errors** (`WebhookError`, `#[non_exhaustive]`; `?` converts it into a handler error):

| Variant | When |
| --- | --- |
| `Invalid(UiError)` | Limits, the Upload Budget, WH-02 (`StaticMessage::try_from`), WH-04, WH-05; nothing sent (except Discord's own 220001/220003) |
| `Encode { reason }` | a Route `custom_id` could not be resolved |
| `RefStore(PortError)` / `RefCollision` | a Ref could not be stored or minted |
| `InvalidToken` | the token is not 1–512 characters of `[A-Za-z0-9._-]` |
| `RateLimited(RateLimited)` | Discord rate limited the request (RL-04); never retried, wait at least `retry_after` |
| `Rejected { status, code }` | Discord refused (403 code 50013 without `MANAGE_WEBHOOKS`, 404 code 10015 for a deleted webhook: create a new one and store it) |
| `Transport` | no usable answer |
| `Detached` | a detached test context; nothing sent |

A webhook failure in a side effect (an audit log, a mirror) should usually not fail the
interaction: log it with `tracing::warn!` (without the token) and carry on.

**Testing.** `TestApp` answers Discord through its HTTP fake; script the webhook routes
with `app.http_fake()` and the types of `noctis::testkit::fake`:

```rust
use noctis::testkit::TestApp;
use noctis::testkit::fake::{FakeResponse, Method, StatusCode};
use serde_json::json;

let app = TestApp::build(&mut builder).await?;
app.http_fake()
    .route(
        Method::POST,
        "/channels/50/webhooks",
        FakeResponse::json(StatusCode::OK, &json!({"id": "900", "token": "tok", "channel_id": "50"})),
    )
    .route_with(Method::POST, "/webhooks/900/tok", |request| {
        assert_eq!(request.query.as_deref(), Some("wait=true&with_components=true"));
        assert_eq!(request.json().unwrap()["flags"], 32768);
        FakeResponse::json(StatusCode::OK, &json!({"id": "71", "channel_id": "50", "attachments": []}))
    });
```

Paths are given without `/api/v10`; `RecordedRequest` exposes `method`, `path` (with the
prefix), `query`, `headers` and `json()`.
