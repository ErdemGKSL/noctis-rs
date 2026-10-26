# Coming from tescord, dbi, serenity or poise

This guide maps what you already know from tescord, dbi (`@mostfeatured/dbi`), serenity
and poise to the Noctis item that does the same job. Each row names the Noctis item;
the linked reference of the Usage Skill documents it in full.

The Usage Skill lives in the project template at
[`examples/project-template/.claude/skills/noctis/`](../../examples/project-template/.claude/skills/noctis/SKILL.md).
Coding agents read the same map in its
[`references/concept-map.md`](../../examples/project-template/.claude/skills/noctis/references/concept-map.md).

Three ideas cover most of the differences:

- **One struct per interaction.** A slash command, a button, a select menu, a modal, an
  event handler and a lifecycle hook are each one struct with a derive, and its
  `#[handler]` method does the work. A Module registers it with `.add::<T>()`.
- **State lives in the `custom_id`, not in memory.** The fields of a component struct
  are its typed Payload, encoded into the `custom_id` and checked at compile time
  against Discord's 100-character limit. A Payload that is too large is stored as a
  Ref with a TTL.
- **Noctis picks the response type.** A handler returns a Components V2 message and
  calls `ctx.respond(..)` or `ctx.update(..)`. Deferring and the Components V2 flag
  are automatic, and ephemerality is declared once on the attribute
  (`response = ephemeral`).

## tescord / dbi → Noctis

| tescord / dbi | Noctis | Reference |
| --- | --- | --- |
| `Tescord` root | `App`, built with `App::from_env()?` (or `App::builder()`) and `.build()?` | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| `Pack` | `Module`: `Module::new("feature").add::<Cmd>().add::<Btn>()`, nested with `.module(..)`, mounted with `AppBuilder::module` | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| dbi `dbi.register(({ ChatInput, Button, Event, Locale }) => { .. })` | one Module per feature, in `src/modules/<feature>/` | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| `pack.slashCommand({ name: "a (b\|c)", handle })`, dbi `ChatInput({ name, onExecute })` | `#[derive(SlashCommand)] #[command("a (b\|c)")] struct A;` plus an `impl A { #[handler] async fn handle(self, ctx: CommandCtx, path: APath) .. }`; the description comes from `cmd.<path>.description` in `locales/en-US.yaml` | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| `options: { user: { type: "User" } }` | fields of the command struct: `struct Ban { #[option(..)] user: ResolvedUser }` | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| `autoComplete` | `#[option(.., autocomplete)]` plus an `#[autocomplete(field)]` method that returns `Suggestions<T>`; Noctis sends the result | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| `pack.button({ id, options, handle })`, dbi `Button({ name, onExecute })` | `#[derive(Button)] #[button("id", defaults(..))] struct X { .. }` plus `impl X { #[handler] async fn handle(self, ctx: ComponentCtx) .. }` | [routes-payloads-refs.md](../../examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md) |
| `buildComponent({ id, data, overrides })`, dbi `button.toJSON({ reference: { data } })` | a typed Payload: the struct's fields are the data, built with `X { .. }.build()`, then a setter per override (`.label(..)`) | [routes-payloads-refs.md](../../examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md) |
| `data[i] as number` | typed Payload fields, decoded for you before the handler runs | [routes-payloads-refs.md](../../examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md) |
| `pack.stringSelect` / `userSelect` / … / `modal` | `#[derive(StringSelectMenu)]`, `UserSelectMenu`, `RoleSelectMenu`, `MentionableSelectMenu`, `ChannelSelectMenu`, `Modal` | [components-v2.md](../../examples/project-template/.claude/skills/noctis/references/components-v2.md), [modals.md](../../examples/project-template/.claude/skills/noctis/references/modals.md) |
| dbi references (objects kept in memory) | `by_ref` Routes: the Payload is stored in the `RefStore` port with a TTL, and the `custom_id` carries a short Ref | [routes-payloads-refs.md](../../examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md) |
| `pack.event({ name, handle })` | `#[derive(EventHandler)] struct X;` plus `impl X { #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) .. }`; the intents are derived from the handlers | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| lifecycle listeners (`ready`, shard state, shutdown) | `#[derive(Hook)]` structs whose handler takes `AllShardsReady`, `ShardFatal`, `ShutdownStarted`, … (`noctis::lifecycle`) | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| `renderSetupMenu(..)` + `deferUpdate()` + `editReply({ components, flags })` | a pure `render_x(..) -> V2Message` function and `ctx.update(render_x(..))`; flags and deferral are automatic | [responder.md](../../examples/project-template/.claude/skills/noctis/references/responder.md), [views-and-recipes.md](../../examples/project-template/.claude/skills/noctis/references/views-and-recipes.md) |
| `locale.user.x.y(a)` | the User Locale: `Strings::of(ctx.user_locale()).x.y(a)`, with typed accessors generated by `i18n!` from `locales/*.yaml` | [i18n.md](../../examples/project-template/.claude/skills/noctis/references/i18n.md) |
| `locale.guild.x.y(a)` | the Guild Locale: `Strings::of(ctx.guild_locale()).x.y(a)` | [i18n.md](../../examples/project-template/.claude/skills/noctis/references/i18n.md) |
| `*.interactions.yaml` `names:`, dbi `InteractionLocale` | `cmdword.<word>` and `cmd.<path>.*` keys in the same locale files | [i18n.md](../../examples/project-template/.claude/skills/noctis/references/i18n.md) |
| `beforeInteraction` returning `false` or an object | a before-Middleware returning `Flow::Stop`, or inserting a typed `Ext<T>` the handler takes as a parameter | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| dbi `rateLimits: [{ type: "User", duration }]` | `rate_limit(user, n, per = "10s")` on the kind attribute | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| `--publish [--guildId]`, dbi `dbi.publish("Guild", id)` | `--publish`, `--publish-guild [<id>]`, `--publish-clear`, `--dry-run`; in code `app.publish(PublishScope::..)` | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| `--features a,b` | Boot Flags: `Module::flags([..])` or `flags = [..]` on a kind attribute, selected with `--features`, `NOCTIS_FEATURES` or `.boot_flags(..)` | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| dbi `CustomEvent` / `dbi.emit(..)` | plugin events: a type implementing `ExtensionEvent`, fired through the App's `Emitter` and handled by `#[derive(Hook)]` structs | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| in-memory `Map` stores | `ctx.kv()` (the `Kv` store over the `CacheStore` port) or your own Adapter; store ids as `u64`, not twilight `Id` | [adapters.md](../../examples/project-template/.claude/skills/noctis/references/adapters.md) |

## serenity / poise → Noctis

| serenity / poise | Noctis | Reference |
| --- | --- | --- |
| framework setup: `serenity::Client::builder(token, intents)`, `poise::Framework::builder()` | `App::from_env()?` (reads `NOCTIS_TOKEN` and the other `NOCTIS_*` variables), then `.module(..)` and `.build()?`; intents are derived from the handlers | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| `impl EventHandler for Handler { async fn message(&self, ctx, msg) }` | one `#[derive(EventHandler)]` struct per event, whose handler takes the typed payload (`MessageCreate`) | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| `#[poise::command(slash_command)] async fn ping(ctx: Context<'_>)` | `#[derive(SlashCommand)] #[command("ping")] struct Ping;` plus `impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> }` | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| poise function arguments as options | fields of the command struct; option descriptions in `cmd.<path>.opt.<name>.description` | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| `ctx.say(..)`, `CreateReply::default().embed(..)` | `ctx.respond(render_x(..))` with a Components V2 message, or `ctx.reply_text(key)` for a short notice; no embeds | [responder.md](../../examples/project-template/.claude/skills/noctis/references/responder.md) |
| `ctx.defer()` | automatic: the auto-defer acknowledges a handler that has not answered in time | [responder.md](../../examples/project-template/.claude/skills/noctis/references/responder.md) |
| `Context<Data>` / `ctx.data()` | `AppBuilder::state(..)`, read with `ctx.state::<T>()` (an `Option<State<T>>`), or a Middleware `Ext<T>` per run | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| `ComponentInteractionCollector`, `await_component_interaction` | a Route: one component struct with its own `#[handler]`; the state travels in its Payload or a Ref | [routes-payloads-refs.md](../../examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md) |
| `poise::builtins::register_in_guild` / `register_globally` | Publish: `--publish-guild [<id>]` / `--publish`, never on READY | [commands.md](../../examples/project-template/.claude/skills/noctis/references/commands.md) |
| poise checks, `required_permissions` | `permissions = ..` on `#[command]`, Route Guards and Middleware | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| poise cooldowns | `rate_limit(..)` on the kind attribute | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| `on_error` in `FrameworkOptions` | Error Sinks (`.error_sink(ErrorOrigin::Command, ..)`) and `HandlerError::user(key)` for a localized notice | [modules-middleware-ratelimits.md](../../examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md) |
| `serenity::cache::Cache` | `ctx.cache()`, the `DiscordCache` port, which returns owned values | [adapters.md](../../examples/project-template/.claude/skills/noctis/references/adapters.md) |
| `ShardManager` | built-in sharding; several processes through Shard Ranges ([Cluster deployment](cluster.md)) | [app-and-sharding.md](../../examples/project-template/.claude/skills/noctis/references/app-and-sharding.md) |
| songbird setup | the `voice` feature and `ctx.voice()` ([Voice and music bots](voice.md)) | [voice.md](../../examples/project-template/.claude/skills/noctis/references/voice.md) |

A whole poise command and its button, in Noctis:

```rust
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("counter")]
pub struct Counter;

impl Counter {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        ctx.respond(render_counter(0)).await
    }
}

#[derive(Button)]
#[button("counter.add", defaults(label = "+1"))]
pub struct Add {
    pub value: u32,
}

impl Add {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render_counter(self.value + 1)).await
    }
}

pub fn module() -> Module {
    Module::new("counter").add::<Counter>().add::<Add>()
}
```

`render_counter(value)` builds the message and places `Add { value }.build()` in it,
and `cmd.counter.description` in `locales/en-US.yaml` describes the command. The click
needs no stored state and survives restarts: the next value is in the button's
`custom_id`.

## What Noctis does not do

These are deliberate. The recipe on the right replaces each one.

| Not supported | Write instead |
| --- | --- |
| Collectors and `await`-loops over component interactions (`await_component_interaction`, `ComponentInteractionCollector`) | one struct with a `#[handler]` per component; the state goes in its Payload, a Ref or a port |
| Inline listeners registered from inside a handler | a Route with a `one_shot` Ref for a single-use button |
| Framework widgets (paginators, confirm dialogs, menus with their own event loop) | the tested recipes (tabs, pagination, step flows, settings) in [views-and-recipes.md](../../examples/project-template/.claude/skills/noctis/references/views-and-recipes.md) |
| Embeds as interaction output | a Components V2 message (`V2Message`) built by a render function |
| Hand-written `custom_id` strings | a component derive and `X { .. }.build()` |
| Publishing commands on READY | `--publish` from the deploy step, or one process with `NOCTIS_PUBLISHER=true` |
| A token or other secret in a config file | `NOCTIS_TOKEN` and the other `NOCTIS_*` environment variables |

The full list of patterns and anti-patterns is the skill's
[`references/patterns.md`](../../examples/project-template/.claude/skills/noctis/references/patterns.md).
