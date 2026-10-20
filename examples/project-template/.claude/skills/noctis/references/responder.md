# Responses and the Responder

Covers: what a handler may return, `IntoResponse`, the context methods
`ctx.respond` / `ctx.update` / `ctx.reply_text`, auto-defer, ephemerality, the
typestate `Responder`, follow-ups, modals, errors shown to users, token expiry,
`LegacyMessage`, webhooks. Rules: INT-01..13, WH-01..05, MSG-05, MSG-07, CV2-01/02.

**Default: build the message with your render function and send it with
`ctx.respond(msg)` or `ctx.update(msg)` (§1b) [planned, Story 8.1]; today a Command
returns the message.** Write no response-type code. Use the typestate Responder only for
modals, follow-ups, or replies that need several messages. A View is just a function
returning a `V2Message` (`views-and-recipes.md`); there is no View trait.

## 1. Return values (`IntoResponse`)

A Command handler (`#[command]`, `#[user_command]`, `#[message_command]`) returns
`HandlerResult<R>` with `R: IntoResponse<Command>` (sealed):

| Handler returns | Command | Component / modal submit [planned, Epic 7] |
| --- | --- | --- |
| `V2Message` or `Components` (`noctis::ui`) | new message (public, or ephemeral if declared) | — |
| `V2Message` from a component or modal handler [planned, Epic 7] | — | **updates the message in place** (or a new message with `response = ephemeral \| public` on its kind attribute) |
| `LegacyMessage` | plain `content` notice (not for interactive UI, UD-1) | typed error if it would turn a V2 message back to legacy |
| `()` | **does not compile**: a command must answer (E0277 `` `()` is not a Command response ``, pointing at the return type) | acknowledges with a deferred update; the message stays as it is |
| `Responded` (from `acked.done()`) | you answered through the Responder | same |
| `Err(HandlerError)` | error path (§5) | error path |

Any other type is the same E0277 (`` `u8` is not a Command response ``, label: return a
`V2Message`, `Components`, a `LegacyMessage`, or the `Responded` from `done()`).

Noctis picks the callback type: a returned message is callback type 4 while the
interaction is unanswered, and an edit of the original (`PATCH …/messages/@original`)
after the handler deferred or replied (INT-12). It adds `IS_COMPONENTS_V2` to every V2
send and edit (CV2-01) and runs the Limits validator (`AppBuilder::limits`, Strict by
default; Lenient truncates with `…`) before any HTTP call. The default `Mentions` ping
nobody (MSG-05). [planned] Finalizing Refs (Story 7.5) and uploading attachments (Story
9.1): a message with attachments fails with `ResponderError::AttachmentsUnsupported`
today, before any request.

A returned message that cannot be sent (invalid, rejected) is a handler error: it goes to
the `Command` Error Sink (§5).

## 1b. `ctx.respond`, `ctx.update`, `ctx.reply_text` [planned, Story 8.1]

The handler context answers in one call and picks the callback type, deferral, flags
and ephemerality itself. Each returns `HandlerResult<Responded>`; return it.

```rust
#[derive(SlashCommand)]
#[command("dashboard", description = "Open the server dashboard", contexts = [Guild])]
pub struct Dashboard;

impl Dashboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx, db: State<Db>) -> HandlerResult<Responded> {
        let data = db.dashboard(ctx.guild_id()).await?;
        ctx.respond(render_dashboard(&data, &Strings::of(ctx.response_locale()), DashTab::Overview)).await
    }
}

impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, db: State<Db>) -> HandlerResult<Responded> {
        if !db.owns(ctx.guild_id(), self.guild).await? {
            return ctx.reply_text(keys::NOT_FOUND).await;          // ephemeral one-liner
        }
        let data = db.dashboard(Some(self.guild)).await?;
        ctx.update(render_dashboard(&data, &Strings::of(ctx.response_locale()), self.tab)).await
    }
}
```

| Method | On | Sends |
| --- | --- | --- |
| `ctx.respond(msg)` | every interactive context | a new message: the initial response (type 4, declared ephemerality) while unanswered; the edit of the deferred original after a Command's auto-defer; a follow-up (declared visibility) once answered |
| `ctx.update(msg)` | `ComponentCtx`, `ModalSubmitCtx` (not `CommandCtx`: E0599, INT-04) | an edit of the message the component lives on: type 7 while unanswered, `edit_reply` after the type-6 auto-defer. On a submit of a modal opened from a slash command (no message to edit) it returns `ResponderError::UpdateUnavailable` and sends nothing, exactly like the typestate `update`; it never falls back to a new message, so answer such a submit with `ctx.respond(msg)` |
| `ctx.reply_text(text)` | every interactive context | one Text Display, **ephemeral** unless `.public()`; the initial response, or an ephemeral follow-up once answered or deferred. `text` is a `String`, a `&'static str` or a `MessageKey` (localized) |

All three add `IS_COMPONENTS_V2`, validate against the Limits and ping nobody (MSG-05).
A second initial response through the ctx methods or a typestate Responder in the same
handler is `ResponderError::AlreadyResponded`, with no request. The doctrine lint
(`xtask skill-check`) accepts these three calls in handlers and rejects
`.reply(`/`.update(`/`.defer_reply(`/`.defer_update(`/`.edit_reply(` on any other receiver
(this release's lint checks the shipped names `.defer(`/`.edit_original(`; Story 6.10
switches it).

## 2. Auto-defer (FR-21, INT-01)

If your Command handler hasn't answered `ack_deadline` after the gateway received the
interaction (queueing time counts), Noctis defers it for you:

- set it with `App::builder().ack_deadline(Duration::from_millis(1500))`: 0 ms..=2500 ms,
  default 2000 ms (`App::ack_deadline()` reads it); out of range is
  `BuildViolation::InvalidAckDeadline` at `build()`; 0 always defers. There is no
  environment variable for it.
- commands: exactly `{"type":5}`, or `{"type":5,"data":{"flags":64}}` when the Command is
  declared `ephemeral` (INT-08); the V2 flag goes on the later edit;
- [planned, Epic 7] components and component-opened modal submits: type 6 (no loading
  state) with the default `response = update`, type 5 with `response = ephemeral |
  public`.

The deadline is App-wide; whether a handler is auto-deferred at all is per handler
(`auto_defer`, §2b).

One gate serializes every initial response, so at most one callback is ever sent
(INT-03): whichever of your handler and the timer gets there first answers. After the
auto-defer, your `reply(msg)` or returned message becomes an edit of the original
(`PATCH …/messages/@original`, `reply` still returns `Ok(Acked)`), and a late
`defer_reply(..)` sends nothing. A handler that answers after 5 s still delivers, without
"interaction failed". The timer stops when your handler returns, fails, panics or is
cancelled.

Handlers declared `auto_defer = false` are never auto-deferred (a modal must be the
first response, INT-05); if one has not answered within the 3 s deadline (2.9 s), Noctis
logs one `error` on target `noctis::dispatch` naming `handler` and `module`.

## 2b. `auto_defer` and `response`: pick per handler [planned: Story 6.10]

Every interaction handler declares two things on its own attribute, both checked at
compile time:

- `auto_defer = true | false` (default `true`): whether Noctis defers it at the
  deadline. Allowed on `#[command]`, `#[user_command]`, `#[message_command]` and on a
  component's or modal's kind attribute (`#[button(..)]`, `#[string_select(..)]`, …,
  `#[modal(..)]`; [planned, Story 7.2/7.8]). `#[handler]` itself takes no arguments.
  Never on `#[autocomplete]` (no defer exists for it: `[INT-06]` compile error).
- `response = …`: what the (auto-)defer and the default reply look like. Commands and
  context menus: `public` (default) or `ephemeral`. Components and component-opened
  modal submits: `update` (default, edits the message in place), `ephemeral` or
  `public` (a new message). A modal opened from a slash command has no message to
  update, so its submit handler should declare `public` or `ephemeral` (a declared
  `update` there auto-defers as type 5 public, and both `update(..)` and `ctx.update(..)`
  return `UpdateUnavailable` without sending anything: answer with `ctx.respond`).

`response` is independent of `auto_defer`: with `auto_defer = false` it still sets what
`reply(msg)` and `defer_reply()` use; `reply_as(msg, e)` and `defer_reply_as(e)`
override it for one call.

```rust
#[derive(SlashCommand)]
#[command("settings", description = "Server settings", response = ephemeral)]
pub struct Settings;                         // auto-defer: type 5 + EPHEMERAL

#[derive(Button)]
#[button("mod.ban", response = ephemeral, auto_defer = true, defaults(style = Danger, label = "mod.ban"))]
pub struct Ban { pub user: Id<UserMarker> }  // a click opens a new private panel

#[derive(Button)]                            // a button that opens a modal
#[button("profile.bio", auto_defer = false, defaults(style = Primary, label = "profile.edit_bio"))]
pub struct OpenBioModal { pub user: Id<UserMarker> }

impl OpenBioModal {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(BioForm { user: self.user, bio: None }.build()).await?.done())
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
| is a modal submit | `auto_defer` and `response` as needed | `ModalResponder` or `ModalResponder<Manual>`; never `show_modal` |
| is autocomplete | neither (compile error) | `AutocompleteCtx` only |

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

Shipped names until Story 6.10 (which converts them): this release spells `auto_defer = false` on a Command as
the bare flag `opens_modal` (`CommandHandler::opens_modal()`) and has no component
handlers yet, the `Manual` alias as
`CommandResponder<OpensModal>` (the sealed `ModalMarker`: `NoModal` by default or
`OpensModal`), `show_modal` as `modal`, and `response = ephemeral` as the bare flag
`ephemeral`; `response = …`, `auto_defer` and `Manual` do not compile yet. Story 6.10
renames them with no aliases.

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
  `#[message_command]`; [planned: Story 6.10], today the bare flag `ephemeral`, and
  `ephemeral = true` is a compile error) — private reply (single-user
  UI: settings, setup flows). `CommandHandler::ephemerality()` / `ctx.ephemerality()` read
  it (`Ephemerality::Ephemeral` / `Ephemerality::Public`).
- Commands default to public (shared content).
- [planned, Epic 7] Components default to update-in-place (keep the message's
  visibility); `response = ephemeral` on the kind attribute — the component opens a **new**
  private sub-panel.

`reply` and `defer_reply()` and a returned message use the declared value;
`reply_as(msg, Ephemerality)` and `defer_reply_as(Ephemerality)` take it explicitly;
`follow_up(msg, Visibility)` takes one per follow-up (`Visibility` is an alias of
`Ephemerality`); **no edit takes one** (`edit_reply(msg, Ephemerality::…)` is E0061).
[planned: Story 6.10; this release spells `defer_reply_as(e)` as `defer(e)`,
`follow_up` as `followup` and `edit_reply` as `edit_original`.]

Locale follows visibility: `ctx.response_locale()` gives the User Locale (then the Guild
Locale) for ephemeral Commands and the Guild Locale (then the User Locale) for public
ones, `"en-US"` when Discord sent neither. Pass `Strings::of(ctx.response_locale())` to
your render function.

## 3b. Interaction-response flows at a glance (discord.js names)

Each method is its own flow, not an alias. Names follow discord.js. [planned: Story 6.10
renames the shipped command methods (`defer` → `defer_reply`, `modal` → `show_modal`,
`edit_original` → `edit_reply`, `delete_original` → `delete_reply`, `followup` →
`follow_up`); Story 7.2 adds the component and modal-submit responders.]

| Method | discord.js | Callback / route | Effect | Use when |
| --- | --- | --- | --- | --- |
| `reply(msg)` / `reply_as(msg, eph)` | `reply` | type 4 | sends a **new** message (the declared `response`, or `eph`) | answering a command, or a sub-panel off a component |
| `defer_reply()` / `defer_reply_as(eph)` | `deferReply` | type 5 | new "is thinking…" message, filled later by `edit_reply` | the new message needs > 2 s of work |
| `update(msg)` | `update` | type 7 | edits **the clicked component's message** in one request | every button/select that re-renders its own screen (the default) |
| `defer_update()` | `deferUpdate` | type 6 | nothing visible; stops the click spinner | the re-render needs > 2 s; then `edit_reply` |
| `show_modal(m)` | `showModal` | type 9 | opens a modal | `auto_defer = false` handlers only (`Manual` responder); first response only: impossible after any ack, including a defer; never from a modal submit |
| `edit_reply(msg)` | `editReply` | `PATCH …/messages/@original` | edits `@original`: the reply after `reply`/`defer_reply`, the component's message after `update`/`defer_update` | after any ack |
| `fetch_reply()` / `delete_reply()` | `fetchReply` / `deleteReply` | `GET` / `DELETE …/messages/@original` | | after any ack |
| `follow_up(msg, vis)` | `followUp` | `POST /webhooks/{app}/{token}` | another new message | after any ack |

A modal can't follow a defer, so only handlers declared `auto_defer = false` (taking the `Manual` responder, §2b) have `show_modal`; they get no auto-defer and must call it within 3 s: open the modal first and do slow work after the submit, never before it.

Equivalences: `defer_update()` + `edit_reply(msg)` ends like `update(msg)`, but takes two
requests, so prefer `update` when the render is fast. `defer_reply(e)` + `edit_reply(msg)`
ends like `reply(msg)` with ephemerality `e`. You rarely call `defer_*` yourself: the
auto-defer fires at 2000 ms (type 6 for components, type 5 for commands).

## 4. The Responder (typestate, INT-03/04)

The type is `Responder<K, S = Pending>` today: `K` is the handler kind (`Command<M>`,
sealed `ResponderKind`; `M` is the sealed `ModalMarker`), `S` the state (`Pending` or
`Acked`, sealed `ResponderState`). [planned: Story 6.10] it becomes `Responder<K, S =
Pending, D = AutoDefer>` with `K` = `Command` (later `Component`, `ModalSubmit`) and the
sealed defer mode `D` = `AutoDefer` (default) or `Manual` (§2b). Take it
as a handler parameter through its alias, right after `ctx` (and the path), before the
options: `(ctx[, path][, r: CommandResponder][, options])`, or `(ctx, target[, r:
CommandResponder])` on a context menu. Any other order, or two responders, is a compile
error listing these forms. [planned: Story 6.11] the options become `self`, so the
method is `handle(self, ctx[, path][, r])` / `handle(self, ctx, target[, r])`. A handler that returns a message takes none (and if it takes
one but never uses it, nothing is sent from it).

| Alias | Pending methods | Kind |
| --- | --- | --- |
| `CommandResponder` (auto-defer) | `reply(msg)`, `reply_as(msg, e)`, `defer_reply()`, `defer_reply_as(e)` (today `defer(e)`) — **no** `show_modal` | chat input / context menu |
| `CommandResponder<Manual>` [planned: Story 6.10; today `CommandResponder<OpensModal>`] | the above plus `show_modal(m)`; required exactly when the command is `#[command(.., auto_defer = false)]` (`[INT-05]` compile error otherwise) | command that opens a modal |
| `ComponentResponder` [planned, Epic 7] | `reply(msg)`, `update(msg)`, `defer_update()`, `defer_reply()` — **no** `show_modal` | buttons, selects |
| `ComponentResponder<Manual>` [planned, Epic 7] | the above plus `show_modal(m)`; required exactly when the component declares `auto_defer = false` | component that opens a modal |
| `ModalResponder` / `ModalResponder<Manual>` [planned, Epic 7] | `reply(msg)`, `update(msg)`, `defer_update()`, `defer_reply()` — **never** `show_modal` | modal submit |

Autocomplete has no Responder: `AutocompleteCtx` offers only
`ctx.respond(choices).await? -> Responded` (`commands.md` §4) [planned, Story 6.7].

Every Pending method consumes the responder and returns `Responder<K, Acked>`:

```rust
r.reply(msg).await?                     -> Responder<Command, Acked>  // callback type 4, declared ephemerality
r.defer_reply().await?                  -> Responder<Command, Acked>  // callback type 5, declared ephemerality; only EPHEMERAL (INT-08)
r.defer_reply_as(Ephemerality::Ephemeral).await? -> Responder<Command, Acked>  // per-call override
r.show_modal(m).await?                  -> Responder<Command, Acked, Manual> // Manual responder only: callback type 9
acked.edit_reply(msg).await?            -> ()
acked.follow_up(msg, Visibility::Public).await? -> FollowupHandle
acked.delete_reply().await?             -> ()
acked.done()                            -> Responded   // return this from the handler
// [planned: Story 6.10] this release spells these defer(e), modal(m) (on
// CommandResponder<OpensModal>), edit_original, followup and delete_original,
// and has no defer_reply(), reply_as or fetch_reply.
handle.message_id()                     -> Id<MessageMarker>
handle.edit(msg).await?                 -> ()
handle.delete().await?                  -> ()
```

`msg` is anything `Into<OutgoingMessage>`: `V2Message`, `Components` or `LegacyMessage`
([planned] `StaticMessage`). Calling a method that doesn't exist for the kind or
state is a compile error: a second `reply` (E0599), `update` on a `CommandResponder`
(E0599), `show_modal` on an auto-defer responder (`[INT-05] show_modal needs auto_defer
= false on this handler`; today `modal` without `OpensModal` is E0599),
`defer_reply_as(CallbackFlags::EPHEMERAL)` (E0308). `Responder`,
`Responded` and `FollowupHandle` are `#[must_use]`; `Debug` never prints the token.

Edits keep the format rules: editing a legacy original into a V2 message sends the
legacy clears (CV2-02); a V2 original edited with a `LegacyMessage` is
`ResponderError::Invalid(UiError::V2ToLegacy)` without a request.

Every call returns `Result<_, ResponderError>` (`#[non_exhaustive]`; `?` converts it into
`HandlerError`):

| Variant | When (nothing is sent unless noted) |
| --- | --- |
| `Invalid(UiError)` | Limits violation (Strict) or a V2 → legacy edit |
| `Encode { reason }` | the body cannot be serialized (an unresolved Route `custom_id`) |
| `AttachmentsUnsupported` | the message has attachments ([planned, Story 9.1]) |
| `EphemeralUnsupported` | an ephemeral response over a Dispatcher message reply (§4.1) |
| `InvalidToken` | the interaction token is not 1-512 characters of `[A-Za-z0-9._-]` |
| `AckDeadlineMissed(AckDeadlineMissed)` | the initial response came after 2.9 s (INT-01) |
| `Rejected { status, code }` | Discord answered an error (`code`: its JSON error code, e.g. `Some(10015)`); a callback answered 40060 (already acknowledged) counts as success |
| `Transport` | no usable response |
| `TokenExpired` | 15 min after the interaction arrived: a follow-up, or an edit/delete of an ephemeral message or of one whose id is unknown (INT-02) |
| `FollowupCap` | the 6th follow-up of a user-install-only interaction (INT-11) |
| `ModalUnavailable` | `show_modal(m)` after another initial response (an auto-defer cannot happen for `auto_defer = false`), or over the `Dispatcher` |

Runtime guards the types can't express:
- if the auto-defer already fired, your `reply` becomes `edit_reply` transparently
  (§2);
- follow-ups are counted when `authorizing_integration_owners` has a user install and no
  guild install: at most 5, the 6th is `ResponderError::FollowupCap` with no request
  (INT-11). A follow-up that fails validation uses no slot; one Discord rejects still
  uses its slot. Guild installs have no cap;
- 15 minutes after the gateway received the interaction the token is expired (INT-02):
  `edit_reply`, `delete_reply`, `handle.edit` and `handle.delete` on a **public**
  message whose id is known fall back to the bot-token channel routes
  (`PATCH`/`DELETE /channels/{channel}/messages/{id}`; a user-installed app outside the
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
        let _ = r.defer_reply().await?.done();                    // this release: r.defer(Ephemerality::Public)
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
        let form = BioForm { user: ctx.user_id(), bio: None, pronouns: None };
        Ok(r.show_modal(form.build()).await?.done())
    }
}
```

[planned: Story 6.10] Until then write `opens_modal`, `CommandResponder<OpensModal>` and
`r.modal(..)` (§2b).

`show_modal(m)` validates the modal against the Limits (`Invalid`) and encodes it (`Encode`
for an unresolved Route id) before any request.

[planned, Epic 7] Example: a component that opens a modal (its struct declares
`auto_defer = false`):

```rust
#[derive(Button, Clone, Copy)]
#[button("profile.bio", auto_defer = false, defaults(style = Primary, label = "profile.edit_bio"))]
pub struct EditBio { pub user: Id<UserMarker> }

impl EditBio {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(BioForm { user: self.user, bio: None, pronouns: None }.build()).await?.done())
    }
}
```

### 4.1 Running a Command from elsewhere: the `Dispatcher`

`ctx.dispatcher()` on an `EventCtx` gives the App's `Dispatcher` (`Clone`), which runs a
registered Command outside an interaction, for example a prefix command:

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
  an `OptionError` in the Command Error Sink).
- `run_command` resolves the path in the guild's scope, then globally, and runs the
  handler. An unregistered path is `Err(DispatchError::UnknownCommand { path })` with no
  request; a failing handler goes to the `Command` Error Sink, not into the result.
- The Dispatcher enforces none of the Command's `default_member_permissions`, `contexts`
  or `nsfw` metadata (Discord enforces those only for real interactions): check them
  yourself before calling `run_command`.
- The response is a message transport: `reply` and a returned message post a reply to
  the invoking message (`message_reference`, `fail_if_not_exists: false`), `defer_reply_as(Public)`
  shows the typing indicator (the later message is then posted as the reply), edits and
  deletes target that reply, follow-ups are plain channel messages. Anything ephemeral
  (a declared `ephemeral` Command, `defer_reply_as(Ephemeral)`, `follow_up(.., Ephemeral)`) is
  `ResponderError::EphemeralUnsupported`, never a silent public post; `show_modal(m)` is
  `ResponderError::ModalUnavailable`. No auto-defer runs.
- Span `noctis.dispatch{app, guild_id, module, path}`.

## 5. Errors shown to users (FR-22)

```rust
// A known, user-facing condition: localized ephemeral notice, logged at debug.
return Err(HandlerError::user(keys::COMMON_GUILD_ONLY));

// Any error type: `?` converts (impl<E: Error + Send + Sync + 'static> From<E>).
let row = db.load(id).await?;

// An error with a friendly message.
db.save(x).await.map_err(|e| HandlerError::new(e).with_message(keys::SAVE_FAILED))?;
```

Now: a Command handler's `Err`, a panic, or a cancellation at shutdown goes to the
`Command` Error Sink (`ReportKind::Error` / `Panic` / `Cancelled`, with `module`,
`handler` and `event_type = "INTERACTION_CREATE"`; `noctis_handler_errors_total{kind="command"}`)
and nothing is sent to the user. [planned, Story 6.8] Default: the user sees a short
localized ephemeral error (`noctis.error.generic`), and the full error is logged with
interaction context. A panic in a handler is caught and
treated the same way; the shard keeps running. Route errors by origin with Error Sinks
(`modules-middleware-ratelimits.md`). Notices from the "disabled here", rate-limit and
guard stages are ephemeral; after an auto-defer they arrive as an ephemeral follow-up
and the deferred original is deleted.

## 6. `LegacyMessage`

`LegacyMessage::new("…")` for plain notices or prefix commands; the 2000-character limit
(MSG-01) is checked by the runtime validator (`noctis::ui::validate`) and, for a
literal, at `cargo check` by `content!("…")`; see `components-v2.md` §12. Up to 5 legacy Action Rows (CV2-15). Never for
dashboards. Editing a legacy message into a View is supported (content/embeds/poll/sticker_ids
are cleared in the same request);
V2 → legacy is a typed error (CV2-01).

## 7. Webhooks (FR-65) [planned]

```rust
// Application-owned webhook: any V2 message (from your render function).
ctx.webhooks().execute(webhook_id, &token, render_log(&entry, &s)).await?;
ctx.webhooks().edit(webhook_id, &token, message_id, render_log(&entry, &s)).await?;

// A webhook you don't own: only a non-interactive StaticMessage (WH-02), checked by type.
ForeignWebhook::new(url).send(StaticMessage::new(render_case_text(&case, &s))).await?;
```

Noctis always sends `?with_components=true` (WH-01) and only the flags valid for the
endpoint (MSG-07). Forum/media channels need `.thread_name(..)` or `.thread(id)` (WH-05).
Webhook name: 1–80 chars (WH-04).
