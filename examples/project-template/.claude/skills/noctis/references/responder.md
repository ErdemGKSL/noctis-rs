# Responses and the Responder

Covers: what a handler may return, `IntoResponse`, auto-defer, ephemerality, the
typestate `Responder`, follow-ups, modals, errors shown to users, token expiry,
`LegacyMessage`, webhooks. Rules: INT-01..13, WH-01..05, MSG-05, MSG-07, CV2-01/02.

**Default: return a View and write no response code.** Use the Responder only for
modals, follow-ups, or replies that need several messages.

## 1. Return values (`IntoResponse`)

A Command handler (`#[command]`, `#[user_command]`, `#[message_command]`) returns
`HandlerResult<R>` with `R: IntoResponse<Command>` (sealed):

| Handler returns | Command | Component / modal submit [planned, Epic 7] |
| --- | --- | --- |
| `V2Message` or `Components` (`noctis::ui`) | new message (public, or ephemeral if declared) | — |
| a `View` (or `view.at(Nav::…)`, `.with_notice(key)`) [planned, Epic 8] | new message | **updates the message in place** (or a new message with `#[handler(response = ephemeral \| public)]`) |
| a `StaticView` [planned, Epic 8] | new message, no components | updates in place (components removed) |
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

## 2. Auto-defer (FR-21, INT-01)

If your Command handler hasn't answered `ack_deadline` after the gateway received the
interaction (queueing time counts), Noctis defers it for you:

- set it with `App::builder().ack_deadline(Duration::from_millis(1500))`: 0 ms..=2500 ms,
  default 2000 ms (`App::ack_deadline()` reads it); out of range is
  `BuildViolation::InvalidAckDeadline` at `build()`; 0 always defers. There is no
  environment variable for it.
- commands: exactly `{"type":5}`, or `{"type":5,"data":{"flags":64}}` when the Command is
  declared `ephemeral` (INT-08); the V2 flag goes on the later edit;
- [planned, Epic 7] components and modal submits: type 6 (no loading state).

One gate serializes every initial response, so at most one callback is ever sent
(INT-03): whichever of your handler and the timer gets there first answers. After the
auto-defer, your `reply(msg)` or returned message becomes an edit of the original
(`PATCH …/messages/@original`, `reply` still returns `Ok(Acked)`), and a late
`defer(..)` sends nothing. A handler that answers after 5 s still delivers, without
"interaction failed". The timer stops when your handler returns, fails, panics or is
cancelled.

`opens_modal` Commands are never auto-deferred (a modal must be the first response,
INT-05); if one has not answered within the 3 s deadline (2.9 s), Noctis logs one
`error` on target `noctis::dispatch` naming `handler` and `module`.

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

- `#[command("…", ephemeral)]` (also on `#[user_command]` / `#[message_command]`; bare
  flag, no value: `ephemeral = true` is a compile error) — private reply (single-user
  UI: settings, wizards). `CommandHandler::ephemerality()` / `ctx.ephemerality()` read
  it (`Ephemerality::Ephemeral` / `Ephemerality::Public`).
- Commands default to public (shared content).
- [planned, Epic 7] Components default to update-in-place (keep the message's
  visibility); `#[handler(response = ephemeral)]` — the component opens a **new**
  private sub-panel.

`reply` and a returned message use the declared value; `defer(Ephemerality)` takes it
explicitly; `followup(msg, Visibility)` takes one per follow-up (`Visibility` is an alias
of `Ephemerality`); **no edit takes one** (`edit_original(msg, Ephemerality::…)` is
E0061).

Locale follows visibility: `ctx.response_locale()` gives the User Locale (then the Guild
Locale) for ephemeral Commands and the Guild Locale (then the User Locale) for public
ones, `"en-US"` when Discord sent neither. [planned] `ViewCx::locale()` (Epic 8).

## 4. The Responder (typestate, INT-03/04)

The type is `Responder<K, S = Pending>`: `K` is the handler kind (`Command<M>`, sealed
`ResponderKind`; `M` is the sealed `ModalMarker`: `NoModal` by default or `OpensModal`), `S` the state (`Pending` or `Acked`, sealed `ResponderState`). Take it
as a handler parameter through its alias, right after `ctx` (and the path), before the
options: `(ctx[, path][, r: CommandResponder][, options])`, or `(ctx, target[, r:
CommandResponder])` on a context menu. Any other order, or two responders, is a compile
error listing these forms. A handler that returns a message takes none (and if it takes
one but never uses it, nothing is sent from it).

| Alias | Pending methods | Kind |
| --- | --- | --- |
| `CommandResponder` (= `CommandResponder<NoModal>`) | `reply(msg)`, `defer(ephemerality)` | chat input / context menu |
| `CommandResponder<OpensModal>` | the above plus `modal(m)`; required exactly when the command is `#[command(.., opens_modal)]` (`[INT-05]` compile error otherwise) | command that opens a modal |
| `ComponentResponder` [planned, Epic 7] | `reply(msg)`, `update(msg)`, `defer()` | buttons, selects |
| `ComponentResponder<OpensModal>` [planned, Epic 7] | the above plus `modal(m)` | component that opens a modal |
| `ModalResponder` [planned, Epic 7] | `reply(msg)`, `update(msg)`, `defer()` — **no** `modal` | modal submit |

Autocomplete has no Responder: `AutocompleteCtx` offers only the synchronous
`ctx.respond(choices) -> Responded` (no `.await`; `commands.md` §4).

Every Pending method consumes the responder and returns `Responder<K, Acked>`:

```rust
r.reply(msg).await?                     -> Responder<Command, Acked>  // callback type 4, declared ephemerality
r.defer(Ephemerality::Ephemeral).await? -> Responder<Command, Acked>  // callback type 5, only EPHEMERAL (INT-08)
r.modal(m).await?                       -> Responder<Command<OpensModal>, Acked> // CommandResponder<OpensModal> only: callback type 9
acked.edit_original(msg).await?         -> ()
acked.followup(msg, Visibility::Public).await? -> FollowupHandle
acked.delete_original().await?          -> ()
acked.done()                            -> Responded   // return this from the handler
handle.message_id()                     -> Id<MessageMarker>
handle.edit(msg).await?                 -> ()
handle.delete().await?                  -> ()
```

`msg` is anything `Into<OutgoingMessage>`: `V2Message`, `Components` or `LegacyMessage`
([planned] a View, `StaticView`). Calling a method that doesn't exist for the kind or
state is a compile error: a second `reply` (E0599), `update` or `modal` on a
`CommandResponder` without `OpensModal` (E0599), `defer(CallbackFlags::EPHEMERAL)` (E0308). `Responder`,
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
| `ModalUnavailable` | `modal(m)` after another initial response (an auto-defer cannot happen for `opens_modal`), or over the `Dispatcher` |

Runtime guards the types can't express:
- if the auto-defer already fired, your `reply` becomes `edit_original` transparently
  (§2);
- follow-ups are counted when `authorizing_integration_owners` has a user install and no
  guild install: at most 5, the 6th is `ResponderError::FollowupCap` with no request
  (INT-11). A follow-up that fails validation uses no slot; one Discord rejects still
  uses its slot. Guild installs have no cap;
- 15 minutes after the gateway received the interaction the token is expired (INT-02):
  `edit_original`, `delete_original`, `handle.edit` and `handle.delete` on a **public**
  message whose id is known fall back to the bot-token channel routes
  (`PATCH`/`DELETE /channels/{channel}/messages/{id}`; a user-installed app outside the
  guild gets `Rejected`); everything else (follow-ups, ephemeral messages, an original
  that was deferred and never edited) is `ResponderError::TokenExpired` without an HTTP
  request. A returned message past 15 min follows the same rule.

Example: reply privately, then post a public log line.

```rust
#[command("mod warn", description = "Warn a member", permissions = MODERATE_MEMBERS, ephemeral)]
async fn warn(ctx: CommandCtx, r: CommandResponder, opts: WarnOptions) -> HandlerResult<Responded> {
    let case = record_warning(&ctx, &opts).await?;
    let acked = r.reply(warn_result(&case)).await?;           // V2Message or Components
    acked.followup(case_log(&case), Visibility::Public).await?;
    Ok(acked.done())
}
```

Example: defer, work, then return the message (it edits the deferred original):

```rust
#[command("report", description = "Build the weekly report")]
async fn report(ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Components> {
    let _ = r.defer(Ephemerality::Public).await?.done();
    Ok(build_report(&ctx).await?)
}
```

Example: a Command that opens a modal (INT-05). After a modal there is no original
message, so return `done()`:

```rust
#[command("bio", description = "Edit your profile bio", opens_modal, contexts = [Guild])]
async fn bio(ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(r.modal(BioForm::modal(SaveBio { user: ctx.user_id() })).await?.done())
}
```

`modal(m)` validates the modal against the Limits (`Invalid`) and encodes it (`Encode`
for an unresolved Route id) before any request.

[planned, Epic 7] Example: a component that opens a modal (Route declared `opens_modal`):

```rust
#[handler]
async fn edit_bio(ctx: ComponentCtx, e: EditBio, r: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(r.modal(BioForm::modal(SaveBio { user: e.user })).await?.done())
}
```

### 4.1 Running a Command from elsewhere: the `Dispatcher`

`ctx.dispatcher()` on an `EventCtx` gives the App's `Dispatcher` (`Clone`), which runs a
registered Command outside an interaction, for example a prefix command:

```rust
#[event]
async fn prefix(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
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
  the invoking message (`message_reference`, `fail_if_not_exists: false`), `defer(Public)`
  shows the typing indicator (the later message is then posted as the reply), edits and
  deletes target that reply, follow-ups are plain channel messages. Anything ephemeral
  (a declared `ephemeral` Command, `defer(Ephemeral)`, `followup(.., Ephemeral)`) is
  `ResponderError::EphemeralUnsupported`, never a silent public post; `modal(m)` is
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

When a Command handler (`#[command]`, `#[user_command]`, `#[message_command]`) returns
`Err`, panics, or its options fail to decode or validate, the user gets **one short
notice** (a V2 message with one `TextDisplay`), then the error is handled:

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
| deferred (by you or by the auto-defer) | the deferred original is deleted (`DELETE …/messages/@original`; a failure is a `warn` and the notice is still sent), then an ephemeral follow-up (AD-23) |
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
- **Errors and panics**: `noctis_handler_errors_total{kind="command"}` +1 and a report to
  the `Command` Error Sink (`ReportKind::Error` / `Panic`, with `module`, `handler` and
  `event_type = "INTERACTION_CREATE"`). The full error chain is logged once inside the
  `noctis.interaction` span: by the default `LogSink`, or, with a custom `Command` (or
  `Autocomplete`) sink, by `LogSink` right before your sink is called — so a custom sink
  never hides the error from the logs. The shard keeps running (NFR-4).
- **User conditions** (`HandlerError::user(key)` and a failed option validator): one
  `debug` on target `noctis::command` (fields `handler`, `key`); no Error Sink, not
  counted in `noctis_handler_errors_total`. `with_message` does **not** make an error a
  user condition.

The locale of the notice is the User Locale, then the Guild Locale, then `en-US` (an
unparseable code means `en-US`). Built-in `noctis.*` texts are English for every
locale today. Until bot locale files arrive ([planned, Epic 11]), a key of your own
(`keys::SAVE_FAILED`) is not in any catalog: the user sees the generic text and
Noctis logs `warn` "missing message key" (target `noctis::i18n`, field `key`). Use
the reserved keys (`i18n.md` §6) when the exact text matters today.

Logs never contain the token, a user id, option values or the notice text.
Autocomplete never shows a notice (it answers an empty list, `commands.md` §4); event
handlers have no user to notify (`HandlerError::user` there is reported like any other
error). Route errors by origin with Error Sinks (`modules-middleware-ratelimits.md`).
[planned, Epic 7/13] Notices from the "disabled here", rate-limit and guard stages use
the same routing.

## 6. `LegacyMessage`

`LegacyMessage::new("…")` for plain notices or prefix commands; the 2000-character limit
(MSG-01) is checked by the runtime validator (`noctis::ui::validate`) and, for a
literal, at `cargo check` by `content!("…")`; see `components-v2.md` §12. Up to 5 legacy Action Rows (CV2-15). Never for
dashboards. Editing a legacy message into a View is supported (content/embeds/poll/sticker_ids
are cleared in the same request);
V2 → legacy is a typed error (CV2-01).

## 7. Webhooks (FR-65) [planned]

```rust
// Application-owned webhook: any View.
ctx.webhooks().execute(webhook_id, &token, LogView::new(entry)).await?;
ctx.webhooks().edit(webhook_id, &token, message_id, LogView::new(entry)).await?;

// A webhook you don't own: only non-interactive StaticViews (WH-02), checked by type.
ForeignWebhook::new(url).send(CaseLog::new(case)).await?;
```

Noctis always sends `?with_components=true` (WH-01) and only the flags valid for the
endpoint (MSG-07). Forum/media channels need `.thread_name(..)` or `.thread(id)` (WH-05).
Webhook name: 1–80 chars (WH-04).
