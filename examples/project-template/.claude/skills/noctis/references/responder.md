# Responses and the Responder

Covers: what a handler may return, `IntoResponse`, auto-defer, ephemerality, the
typestate `Responder`, follow-ups, modals, errors shown to users, token expiry,
`LegacyMessage`, webhooks. Rules: INT-01..13, WH-01..05, MSG-05, MSG-07, CV2-01/02.

**Default: return a View and write no response code.** Use the Responder only for
modals, follow-ups, or replies that need several messages.

## 1. Return values (`IntoResponse`) [planned]

| Handler returns | Command | Component / modal submit |
| --- | --- | --- |
| a `View` (or `view.at(Nav::…)`, `.with_notice(key)`) | new message (public, or ephemeral if declared) | **updates the message in place** (or a new message with `#[handler(response = ephemeral \| public)]`) |
| a `StaticView` | new message, no components | updates in place (components removed) |
| `LegacyMessage` | plain `content` notice (not for interactive UI, UD-1) | typed error if it would turn a V2 message back to legacy |
| `()` | not allowed: a command must answer (compile error) | acknowledges with a deferred update; the message stays as it is |
| `Responded` | you answered through the Responder | same |
| `Err(HandlerError)` | error path (§5) | error path |

Noctis picks the callback type: command → type 4 (or 5 + edit), component/modal submit
→ type 7 (or 6 + edit). It adds `IS_COMPONENTS_V2` to every V2 send and edit (CV2-01),
finalizes Refs, uploads attachments (diffed on edit), sets `allowed_mentions.parse = []`
for Views unless opted in (MSG-05), and runs the Limits validator before any HTTP call.

## 2. Auto-defer (FR-21)

If your handler hasn't answered `ack_deadline` (default 2000 ms) after the gateway
received the interaction (queueing time counts), Noctis defers:

- commands: type 5 with only `EPHEMERAL` if declared (INT-08); the V2 flag goes on the
  later edit;
- components and modal submits: type 6 (no loading state).

Your eventual return value then becomes an edit of the original. A handler that
answers after 5 s still delivers, without "interaction failed". `opens_modal` handlers
are never auto-deferred (a modal must be the first response, INT-05); if one misses the
3 s deadline, Noctis logs an error naming it.

Rendering images is awaited **inside** the handler (`ctx.render(..).await`), so slow
renders are covered automatically.

## 3. Ephemerality (INT-10)

Declared at registration, never toggled later:

- `#[command("…", ephemeral)]` — private reply (single-user UI: settings, wizards).
- Commands default to public (shared content).
- Components default to update-in-place (keep the message's visibility).
- `#[handler(response = ephemeral)]` — the component opens a **new** private sub-panel
  (e.g. per-user settings from a public dashboard).

Locale follows visibility: `ctx.response_locale()` and `ViewCx::locale()` give the User
Locale for ephemeral responses and the Guild Locale for public ones.

## 4. The Responder (typestate, INT-03/04) [planned]

The type is `Responder<K, S = Pending>`: `K` is the handler kind, `S` the state
(`Pending` or `Acked`). Take it as a handler parameter through an alias; a handler that
returns a View takes none.

| Alias | Pending methods | Kind |
| --- | --- | --- |
| `CommandResponder` (= `CommandResponder<NoModal>`) | `reply(msg)`, `defer(ephemerality)` | chat input / context menu |
| `CommandResponder<OpensModal>` | the above plus `modal(m)`; required when the command is `#[command(.., opens_modal)]` | command that opens a modal |
| `ComponentResponder` (= `ComponentResponder<NoModal>`) | `reply(msg)`, `update(msg)`, `defer()` | buttons, selects |
| `ComponentResponder<OpensModal>` | the above plus `modal(m)`; required when the Route is `#[route(.., opens_modal)]` | component that opens a modal |
| `ModalResponder` | `reply(msg)`, `update(msg)`, `defer()` — **no** `modal` (no modal chaining) | modal submit |

Autocomplete has no Responder: `AutocompleteCtx` offers only
`ctx.respond(choices).await? -> Responded` (`commands.md` §4).

The handler macro checks that the alias matches the declaration: `opens_modal` without
`OpensModal`, or `OpensModal` without `opens_modal`, is a compile error (INT-05).

Every Pending method consumes the responder and returns `Responder<K, Acked>`:

```rust
acked.edit_original(msg).await?        -> Responder<K, Acked>
acked.followup(msg, Visibility::Ephemeral).await? -> FollowupHandle
acked.delete_original().await?
acked.done()                           -> Responded   // return this from the handler
```

`msg` is anything message-like: a View, `StaticView`, `LegacyMessage`. Calling a method
that doesn't exist for the kind is a compile error (e.g. `update` on a command).

Runtime guards the types can't express:
- if the auto-defer already fired, your `reply`/`update` becomes `edit_original`
  transparently;
- follow-ups are counted: max 5 for user-installed apps not in the guild (INT-11) →
  `ResponderError::FollowupCap`;
- after 15 minutes every call returns `ResponderError::TokenExpired` without an HTTP
  request (INT-02). Updates to a *public* View past 15 min fall back to a bot-token
  message edit automatically; ephemeral ones can't.

Example: show a View, then post a public log line.

```rust
#[command("mod warn", permissions = MODERATE_MEMBERS, ephemeral)]
async fn warn(ctx: CommandCtx, opts: WarnOptions, r: CommandResponder) -> HandlerResult<Responded> {
    let case = record_warning(&ctx, &opts).await?;
    let acked = r.reply(WarnResultView::new(case.clone())).await?;
    acked.followup(CaseLog::new(case), Visibility::Public).await?;
    Ok(acked.done())
}
```

Example: open a modal (Route declared `opens_modal`):

```rust
#[handler]
async fn edit_bio(ctx: ComponentCtx, e: EditBio, r: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(r.modal(BioForm::modal(SaveBio { user: e.user })).await?.done())
}
```

## 5. Errors shown to users (FR-22)

```rust
// A known, user-facing condition: localized ephemeral notice, logged at debug.
return Err(HandlerError::user(keys::COMMON_GUILD_ONLY));

// Any error type: `?` converts (impl<E: Error + Send + Sync + 'static> From<E>).
let row = db.load(id).await?;

// An error with a friendly message.
db.save(x).await.map_err(|e| HandlerError::new(e).with_message(keys::SAVE_FAILED))?;
```

Default: the user sees a short localized ephemeral error (`noctis.error.generic`), and
the full error is logged with interaction context. A panic in a handler is caught and
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
// Application-owned webhook: any View.
ctx.webhooks().execute(webhook_id, &token, LogView::new(entry)).await?;
ctx.webhooks().edit(webhook_id, &token, message_id, LogView::new(entry)).await?;

// A webhook you don't own: only non-interactive StaticViews (WH-02), checked by type.
ForeignWebhook::new(url).send(CaseLog::new(case)).await?;
```

Noctis always sends `?with_components=true` (WH-01) and only the flags valid for the
endpoint (MSG-07). Forum/media channels need `.thread_name(..)` or `.thread(id)` (WH-05).
Webhook name: 1–80 chars (WH-04).
